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

use std::collections::{BTreeSet, HashMap, HashSet};

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
    /// The day she learned each memory a head names, Unix microseconds
    /// ([`known::learned`]): of those held, and of those a later fact
    /// replaced, for what rests on one was still said that day.
    pub learned: HashMap<String, i64>,
    /// Those a head names that they took back or had her let go
    /// ([`known::Named::taken_back`]).
    pub taken_back: HashSet<String>,
    /// The words an ended item stood in, by (node, item): the version its
    /// ending was written over, which the head no longer keeps. None of one
    /// whose earlier version is forgotten, or is not to be found.
    pub stood: HashMap<(String, String), String>,
    /// Every memory any version of an item of 「现在」 ever rested on, by
    /// (node, item): a version is written with the one before it in view,
    /// so its words may carry what an earlier one rested on.
    pub grounds: HashMap<(String, String), Vec<String>>,
    /// The memories that showed an item of 「现在」 still holding, by (node,
    /// item), latest learned first (`super::kept`).
    pub kept: HashMap<(String, String), Vec<String>>,
}

/// The words an ended thing stood in, at most: a line says which thing it
/// was, not all of it.
const STOOD_CHARS: usize = 80;

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

    /// When they told her what the memory row `id` keeps, held or since
    /// replaced by a later fact; none for one they took back.
    pub(crate) fn told_of(&self, id: &str) -> Option<i64> {
        (!self.taken_back.contains(id))
            .then(|| self.learned.get(id).copied())
            .flatten()
    }

    /// The latest day `item` of `node`'s 「现在」 was seen to still hold:
    /// when she learned the latest memory that showed it, of those that
    /// still tell when it was said ([`Shown::told_of`]: not taken back,
    /// whether held or since replaced).
    pub(crate) fn still_so(&self, node: &str, item: &str) -> Option<i64> {
        self.kept
            .get(&(node.to_string(), item.to_string()))?
            .iter()
            .filter_map(|id| self.told_of(id))
            .max()
    }

    /// Whether what rests on the memories `because` names is no longer to
    /// be said: they took any one of them back, for its words may carry
    /// what was taken back (the rule forgetting keeps: [`super::forget`]).
    /// One replaced by a later fact is not taken back: what rests on it
    /// stands.
    pub(crate) fn unsaid(&self, because: &Value) -> bool {
        cited(because).iter().any(|id| self.taken_back.contains(id))
    }

    /// Whether `item` of `node`'s 「现在」, resting now on `because`, is no
    /// longer to be said: they took back a memory it rests on, or one any
    /// earlier version of it rested on ([`Shown::grounds`]). This hides an
    /// item whose words were since rewritten clear of what was taken back,
    /// until it is written again: too much hidden, never too little.
    pub(crate) fn unsaid_item(&self, node: &str, item: &str, because: &Value) -> bool {
        self.unsaid(because)
            || self
                .grounds
                .get(&(node.to_string(), item.to_string()))
                .is_some_and(|ids| ids.iter().any(|id| self.taken_back.contains(id)))
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
/// open items, and what stands or ended there (so since when it does, and
/// whether what it rests on is still held, can be told).
fn named_by(head: &Value) -> Vec<String> {
    let entries = ["entries", "spans"]
        .iter()
        .flat_map(|field| head[field].as_array().into_iter().flatten())
        .flat_map(|item| cited(&item["refs"]));
    let items = ["views", "open", "standing"]
        .iter()
        .flat_map(|field| head[field].as_object().into_iter().flatten())
        .flat_map(|(_, item)| cited(&item["value"]["because"]));
    entries.chain(items).collect()
}

/// Of the items of 「现在」 on `heads` (node id first, head last): every
/// memory any version of each ever rested on, and of an ended one the
/// words it stood in. One read, of those nodes and items only; a version
/// forgotten (a stub, no body) gives nothing. A node gone into another
/// keeps its rows under its old id: nothing is found for it, and its ended
/// line says only that it is no longer as it stood.
#[allow(clippy::type_complexity)]
async fn versions<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    heads: &[(String, Option<String>, Option<String>, String, Value)],
) -> Result<
    (
        HashMap<(String, String), Vec<String>>,
        HashMap<(String, String), String>,
    ),
    Error,
> {
    let items = |head: &Value| -> Vec<(String, Value)> {
        head["standing"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(item, held)| (item.clone(), held.clone()))
            .collect()
    };
    let nodes: Vec<String> = heads
        .iter()
        .filter(|head| !items(&head.4).is_empty())
        .map(|head| head.0.clone())
        .collect();
    let names: BTreeSet<String> = heads
        .iter()
        .flat_map(|head| items(&head.4))
        .map(|(item, _)| item)
        .collect();
    let (mut grounds, mut stood) = (HashMap::new(), HashMap::new());
    if nodes.is_empty() {
        return Ok((grounds, stood));
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node_id, row_id, \
               (CASE WHEN body = '' THEN NULL ELSE body::jsonb END) AS body \
             FROM agent_node_rows \
             WHERE user_id = $1 AND audience = $2 AND kind = 'node.standing' \
               AND node_id = ANY($3) \
               AND (CASE WHEN body = '' THEN NULL ELSE body::jsonb->>'item' END) = ANY($4)",
            [
                user_id.into(),
                audience.into(),
                nodes.into(),
                names.into_iter().collect::<Vec<_>>().into(),
            ],
        ))
        .await?;
    // Each version by its row: its node and item, and its body.
    let mut by_row: HashMap<String, (String, String, Value)> = HashMap::new();
    for row in &rows {
        let Some(body) = row.try_get::<Option<Value>>("", "body")? else {
            continue;
        };
        let Some(item) = body["item"].as_str().map(str::to_string) else {
            continue;
        };
        let node: String = row.try_get("", "node_id")?;
        let rests_on: &mut Vec<String> = grounds.entry((node.clone(), item.clone())).or_default();
        for id in cited(&body["because"]) {
            if !rests_on.contains(&id) {
                rests_on.push(id);
            }
        }
        by_row.insert(row.try_get("", "row_id")?, (node, item, body));
    }
    // What an ended item stood in: the version its ending names as the one
    // it was written over (`base`), not whichever was written last.
    for (node, _, _, _, head) in heads {
        for (item, held) in items(head) {
            if held["value"]["ended"].as_bool() != Some(true) {
                continue;
            }
            let over = held["row"]
                .as_str()
                .and_then(|row| by_row.get(row))
                .map(|(_, _, ending)| ending["base"].clone())
                .unwrap_or_default();
            let was = over
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|row| by_row.get(row.as_str()?))
                .filter(|(on, of, body)| {
                    on == node && *of == item && body["ended"].as_bool() != Some(true)
                })
                .filter_map(|(_, _, body)| body["text"].as_str().map(str::trim))
                .find(|text| !text.is_empty());
            if let Some(was) = was {
                let cut: String = was.chars().take(STOOD_CHARS).collect();
                let was = if cut.len() < was.len() {
                    format!("{cut}…")
                } else {
                    cut
                };
                stood.insert((node.clone(), item), was);
            }
        }
    }
    Ok((grounds, stood))
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
    // Every version of what stands or ended on these heads, in one read:
    // what each ever rested on, and of an ended one the words it stood in,
    // which the head no longer keeps.
    let (grounds, stood) = versions(db, user_id, audience, &heads).await?;
    let on: BTreeSet<String> = grounds.keys().map(|(node, _)| node.clone()).collect();
    let kept = super::kept::on_nodes(db, user_id, audience, on.into_iter().collect()).await?;
    let named: BTreeSet<String> = heads
        .iter()
        .flat_map(|head| named_by(&head.4))
        .chain(grounds.values().flatten().cloned())
        .chain(kept.values().flatten().cloned())
        .collect();
    // One read of every memory the heads name: held, taken back, learned.
    let named = known::named(db, named.into_iter().collect()).await?;
    let held: BTreeSet<String> = named
        .iter()
        .filter(|(_, row)| row.held)
        .map(|(id, _)| id.clone())
        .collect();
    let taken_back: HashSet<String> = named
        .iter()
        .filter(|(_, row)| row.taken_back)
        .map(|(id, _)| id.clone())
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
    let learned = named
        .into_iter()
        .map(|(id, row)| (id, row.learned))
        .collect();
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
        taken_back,
        stood,
        grounds,
        kept,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn head(entries: Value) -> Value {
        json!({ "entries": entries })
    }

    /// The memories what stands or ended rests on are read with the head,
    /// though no entry names them: since when it stands is their day.
    #[test]
    fn what_stands_names_the_memories_it_rests_on() {
        let head = json!({
            "entries": [{"refs": ["memory:m1"]}],
            "standing": {"them-1": {"value": {"because": ["memory:m2", "row:r"]}}},
            "views": {"v": {"value": {"because": ["memory:m3"]}}},
        });
        let mut named = named_by(&head);
        named.sort();
        assert_eq!(named, ["m1", "m2", "m3"]);
        let shown = Shown {
            taken_back: ["m2".to_string()].into(),
            ..Default::default()
        };
        // Any it rests on taken back: not said, though another is held.
        // None taken back, or none named: it is.
        assert!(shown.unsaid(&json!(["memory:m2"])));
        assert!(shown.unsaid(&json!(["memory:m1", "memory:m2"])));
        assert!(!shown.unsaid(&json!(["memory:m1"])));
        assert!(!shown.unsaid(&json!([])));
        // Told when it was learned, held or replaced; never when taken
        // back.
        let shown = Shown {
            learned: [("m1".to_string(), 5), ("m2".to_string(), 7)].into(),
            ..shown
        };
        assert_eq!(shown.told_of("m1"), Some(5));
        assert_eq!(shown.told_of("m2"), None);
        assert_eq!(shown.learned_of("m1"), None, "not held: no entry of it");
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
