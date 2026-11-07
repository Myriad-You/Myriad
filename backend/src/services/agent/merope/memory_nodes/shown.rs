//! What of one audience's nodes she may be shown before she speaks: every
//! node as its head stands, each entry sorted by how far she may take it
//! (decision D4, `myriad_nodes::trust`), and the memory rows they name.
//!
//! - **Live only**: an entry whose memory was replaced, faded or let go is
//!   never shown ([`super::known::held`]); one naming no memory stands on
//!   its own.
//! - **By trust**, in a private talk: what counts (the person's own words,
//!   hers) is a fact; what was heard about them from others and they never
//!   confirmed is 「听说」, kept apart; what was only seen, or is not for this
//!   talk at all (`trust::usable_in_private`), is not brought. The site keeps
//!   no provenance of its own yet (who was there, what was typed), so it
//!   gives none: everything here is weighed by who said it and where. In a
//!   group or her own audience everything said there is that audience's
//!   own, brought as it always was there (N4).
//!
//! One read of one account's one audience: two never mix.

use std::collections::{BTreeSet, HashMap};

use myriad_merope::happened::Happened;
use myriad_nodes::{
    SaidBy,
    body::{MEMORY_REF, Status},
    trust::{self, Provenance, Tier},
};
use sea_orm::{ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, QueryFilter, Statement};
use serde_json::Value;

use super::{Error, known};
use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified::KEPT_APART;

/// One entry, as she may be shown it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// The memory row it is the place of, else its own row: the id recall
    /// knows it by.
    pub id: String,
    pub row: String,
    pub memory: Option<String>,
    pub text: String,
    pub said_by: Vec<SaidBy>,
    /// The row's clock, Unix microseconds.
    pub at: i64,
    pub happened: Option<Happened>,
    pub state: Option<String>,
    pub open: Option<bool>,
}

/// One node of the audience.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub id: String,
    /// `topic`, `task` or `person`.
    pub kind: Option<String>,
    pub name: Option<String>,
    /// Absent: active. Only for a node still there.
    pub status: Option<Status>,
    /// Forgotten, taken back, or gone into another.
    pub gone: bool,
    /// The head as the core folded it.
    pub head: Value,
    /// What counts: facts.
    pub facts: Vec<Entry>,
    /// Heard about them, not confirmed: 「听说」.
    pub heard: Vec<Entry>,
    /// Its newest entry or span, Unix microseconds.
    pub latest: i64,
}

/// An audience as she may be shown it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Shown {
    pub nodes: Vec<Node>,
    /// The live memory rows its entries, views and open items name, by id.
    pub memories: HashMap<String, agent_memories::Model>,
    /// The day she learned each of those, Unix microseconds
    /// ([`known::learned`]).
    pub learned: HashMap<String, i64>,
}

impl Shown {
    pub(crate) fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id && !node.gone)
    }

    /// Whether `entry` is an ordinary memory, not one of her other sources
    /// that has a place of its own when she speaks (a bit, what they told
    /// her about herself: `unified::KEPT_APART`).
    pub(crate) fn ordinary(&self, entry: &Entry) -> bool {
        entry
            .memory
            .as_ref()
            .and_then(|id| self.memories.get(id))
            .is_none_or(|row| !KEPT_APART.contains(&row.source.as_str()))
    }

    /// When she learned `entry`: its memory's day when it names one still
    /// held, else its row's clock. Not when it was put on the node.
    pub(crate) fn learned(&self, entry: &Entry) -> i64 {
        entry
            .memory
            .as_ref()
            .and_then(|id| self.learned_of(id))
            .unwrap_or(entry.at)
    }

    /// When she learned the memory row `id`, while it is held.
    pub(crate) fn learned_of(&self, id: &str) -> Option<i64> {
        let row = self.memories.get(id)?;
        Some(
            self.learned
                .get(id)
                .copied()
                .unwrap_or_else(|| row.created_at.timestamp_micros()),
        )
    }

    /// The live memory row an item of a head (a view, an open item) rests
    /// on: the first it names.
    pub(crate) fn memory_of(&self, item: &Value) -> Option<&agent_memories::Model> {
        cited(&item["value"]["because"])
            .iter()
            .find_map(|id| self.memories.get(id))
    }
}

/// The memory rows a list of references names, without `memory:`.
pub(crate) fn cited(references: &Value) -> Vec<String> {
    references
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|reference| reference.as_str()?.strip_prefix(MEMORY_REF))
        .map(str::to_string)
        .collect()
}

/// How far she may take what `said_by` said in `said_in`, shown in
/// `audience`; `about_partner`: it is on 「关于你」 there. `None`: not
/// brought at all.
pub(crate) fn tier_of(
    said_by: &[SaidBy],
    said_in: &str,
    audience: &str,
    about_partner: bool,
    provenance: Option<&Provenance>,
) -> Option<Tier> {
    if !trust::is_private(audience) {
        return Some(Tier::Counts);
    }
    if !trust::usable_in_private(said_by, said_in, audience, provenance) {
        return None;
    }
    trust::tier(said_by, provenance, about_partner)
}

fn said_by_of(value: &Value) -> Vec<SaidBy> {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

/// The entries of `head` on node `about` of `audience`, sorted by trust:
/// (facts, heard). `held`: the memory rows still held; `provenance`: what
/// the host knows of where an entry came from, by its row.
pub(crate) fn sort_entries(
    head: &Value,
    audience: &str,
    about_partner: bool,
    held: &BTreeSet<String>,
    provenance: &HashMap<String, Provenance>,
) -> (Vec<Entry>, Vec<Entry>) {
    let (mut facts, mut heard) = (Vec::new(), Vec::new());
    for entry in head["entries"].as_array().into_iter().flatten() {
        let memories = cited(&entry["refs"]);
        if !memories.is_empty() && !memories.iter().any(|memory| held.contains(memory)) {
            continue;
        }
        let row = entry["row"].as_str().unwrap_or_default().to_string();
        let said_by = said_by_of(&entry["saidBy"]);
        let said_in = entry["audience"].as_str().unwrap_or(audience);
        let memory = memories.into_iter().find(|memory| held.contains(memory));
        let shown = Entry {
            id: memory.clone().unwrap_or_else(|| row.clone()),
            memory,
            text: entry["text"].as_str().unwrap_or_default().to_string(),
            at: entry["at"].as_i64().unwrap_or_default(),
            happened: serde_json::from_value(entry["happened"].clone()).ok(),
            state: entry["state"].as_str().map(str::to_string),
            open: entry["open"].as_bool(),
            row,
            said_by,
        };
        match tier_of(
            &shown.said_by,
            said_in,
            audience,
            about_partner,
            provenance.get(&shown.row),
        ) {
            Some(Tier::Counts) => facts.push(shown),
            Some(Tier::Heard) => heard.push(shown),
            Some(Tier::Seen) | None => {}
        }
    }
    (facts, heard)
}

fn latest_of(head: &Value) -> i64 {
    ["entries", "spans"]
        .iter()
        .flat_map(|field| head[field].as_array().into_iter().flatten())
        .filter_map(|item| item["at"].as_i64())
        .chain(head["createdAt"].as_i64())
        .max()
        .unwrap_or(0)
}

/// Every memory row a head names: on its entries and spans, views and
/// open items.
fn named_by(head: &Value) -> Vec<String> {
    let entries = ["entries", "spans"]
        .iter()
        .flat_map(|field| head[field].as_array().into_iter().flatten())
        .flat_map(|item| cited(&item["refs"]));
    let items = ["views", "open"]
        .iter()
        .flat_map(|field| head[field].as_object().into_iter().flatten())
        .flat_map(|(_, item)| cited(&item["value"]["because"]));
    entries.chain(items).collect()
}

/// `user_id`'s `audience` as she may be shown it; `about`: the node of
/// 「关于你」 there, if it is a private talk.
pub(crate) async fn shown<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    about: Option<&str>,
) -> Result<Shown, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node_id, kind, name, status, node FROM agent_node_heads \
             WHERE user_id = $1 AND audience = $2 ORDER BY node_id",
            [user_id.into(), audience.into()],
        ))
        .await?;
    let mut heads = Vec::new();
    for row in &rows {
        heads.push((
            row.try_get::<String>("", "node_id")?,
            row.try_get::<Option<String>>("", "kind")?,
            row.try_get::<Option<String>>("", "name")?,
            row.try_get::<String>("", "status")?,
            row.try_get::<Value>("", "node")?,
        ));
    }
    let named: BTreeSet<String> = heads.iter().flat_map(|head| named_by(&head.4)).collect();
    let held: BTreeSet<String> = known::held(db, named.into_iter().collect())
        .await?
        .into_iter()
        .collect();
    let memories = if held.is_empty() {
        HashMap::new()
    } else {
        agent_memories::Entity::find()
            .filter(agent_memories::Column::Id.is_in(held.iter().cloned()))
            .all(db)
            .await?
            .into_iter()
            .map(|row| (row.id.clone(), row))
            .collect()
    };
    let learned = known::learned(db, memories.keys().cloned().collect()).await?;
    // The site keeps none yet.
    let provenance = HashMap::new();
    let nodes = heads
        .into_iter()
        .map(|(id, kind, name, status, head)| {
            let gone = !known::is_live(&status);
            let (facts, heard) = if gone {
                (Vec::new(), Vec::new())
            } else {
                sort_entries(
                    &head,
                    audience,
                    about == Some(id.as_str()),
                    &held,
                    &provenance,
                )
            };
            Node {
                status: serde_json::from_value(Value::String(status)).ok(),
                latest: latest_of(&head),
                id,
                kind,
                name,
                gone,
                head,
                facts,
                heard,
            }
        })
        .collect();
    Ok(Shown {
        nodes,
        memories,
        learned,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn head(entries: Value) -> Value {
        json!({ "entries": entries })
    }

    #[test]
    fn in_a_private_talk_only_what_counts_is_a_fact_and_heard_is_apart() {
        let talk = "personal:7";
        let held: BTreeSet<String> = ["m1", "m2", "m3", "m4"].map(String::from).into();
        let head = head(json!([
            {"row": "r1", "text": "养了两只猫", "saidBy": ["partner"], "audience": talk,
             "refs": ["memory:m1"], "at": 1},
            {"row": "r2", "text": "她说他升职了", "saidBy": ["third_party"], "audience": talk,
             "refs": ["memory:m2"], "at": 2},
            {"row": "r3", "text": "群里有人说他胖", "saidBy": ["other_person"], "audience": "group:qq:5",
             "refs": ["memory:m3"], "at": 3},
            {"row": "r4", "text": "换了工作", "saidBy": ["partner"], "audience": talk,
             "refs": ["memory:gone"], "at": 4},
            {"row": "r5", "text": "在群里说自己戒烟了", "saidBy": ["partner"], "audience": "group:qq:5",
             "refs": ["memory:m4"], "at": 5}
        ]));
        let none = HashMap::new();
        let (facts, heard) = sort_entries(&head, talk, true, &held, &none);
        let texts: Vec<&str> = facts.iter().map(|entry| entry.text.as_str()).collect();
        // Their own words, here or about themselves in a group; never a
        // group member's, never one whose memory went.
        assert_eq!(texts, ["养了两只猫", "在群里说自己戒烟了"]);
        assert_eq!(facts[0].id, "m1");
        // Without anyone knowing they were there, someone else's word is
        // only seen: not brought.
        assert!(heard.is_empty());
        // Seen with them, about them: heard, apart.
        let together = HashMap::from([(
            "r2".to_string(),
            Provenance {
                together: true,
                ..Default::default()
            },
        )]);
        let (facts, heard) = sort_entries(&head, talk, true, &held, &together);
        assert_eq!(facts.len(), 2);
        assert_eq!(heard.len(), 1);
        assert_eq!(heard[0].text, "她说他升职了");
        // Not on 「关于你」, the same is only seen.
        let (_, heard) = sort_entries(&head, talk, false, &held, &together);
        assert!(heard.is_empty());
    }

    #[test]
    fn what_names_no_memory_stands_on_its_own() {
        let head = head(json!([
            {"row": "r9", "text": "一起想的名字", "saidBy": ["her", "partner"],
             "audience": "personal:7", "at": 9}
        ]));
        let (facts, _) = sort_entries(
            &head,
            "personal:7",
            false,
            &BTreeSet::new(),
            &HashMap::new(),
        );
        assert_eq!(facts.len(), 1);
        assert_eq!(
            (facts[0].id.as_str(), facts[0].memory.as_ref()),
            ("r9", None)
        );
    }
}
