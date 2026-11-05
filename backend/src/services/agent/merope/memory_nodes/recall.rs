//! What comes to mind of her nodes as she answers, while she reads them
//! (§4 of the plan): the site ranks what she keeps for their words as it
//! always has (`unified::recall`, decision D3); the core picks from that
//! ranking which nodes come, and which entries of each
//! (`myriad_nodes::recall::pick`: three nodes, three entries each, five of
//! 「关于你」); what it ranked and showed on none comes as it did before.
//!
//! - **Facts only**: what she may take as a fact ([`super::shown`]). What
//!   was heard about them from others comes apart, as 「听说」; what was
//!   only seen does not come, though the ranking found it.
//! - **Going through all of a kind** ("which ones", "how many"): code lists
//!   every entry of the nodes of the kind their words found, once each,
//!   in the stretch of time and the state asked about
//!   (`myriad_nodes::listing`), their own words beside a fact where they
//!   add something; instead of the rest of a topic, up to 24, she counts
//!   herself.
//!
//! Pure: the audience as shown, and the ranking, in; lines out.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use myriad_merope::remembering::Cues;
use myriad_nodes::{
    listing::{self, ListEntry, ListInput, ListNode, RangeInput},
    recall::{self, PickEntry, PickInput, PickNode, Ranked},
};

use super::shown::{Entry, Shown, cited};
use crate::services::agent::memory::unified::MemoryRecord;
use crate::services::agent::merope::clock::{self, Zone};
use crate::services::agent::merope::store::{self, Recalled};

/// Heard pieces about them shown, at most (the native budget).
pub(crate) const HEARD: usize = 2;

/// Her own two nodes in the audience, never picked: 「关于你」 and hers.
#[derive(Debug, Clone, Default)]
pub(crate) struct Own {
    pub about: Option<String>,
    pub her: Option<String>,
}

/// In a group: who is talking, and the names the group knows its members
/// by. What is about anyone else there is said as theirs, as recall has
/// always said it (`store::theirs_here`).
#[derive(Debug, Clone, Default)]
pub(crate) struct Here {
    pub speaker: i32,
    pub names: HashMap<i32, String>,
}

impl Here {
    /// `text`, of the memory row `row`, said as whose it is.
    fn theirs(&self, text: String, row: Option<&MemoryRecord>) -> String {
        match row.and_then(|row| row.user_id) {
            Some(other) if other != self.speaker => {
                store::theirs_here(&text, other, Some(&self.names))
            }
            _ => text,
        }
    }
}

/// What comes to mind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Recollection {
    /// What she knows of them, as facts: of the nodes their words touch
    /// (each under its name), what the ranking found on none, and what she
    /// keeps of them as a person.
    pub named: Vec<String>,
    /// 「听说」: heard about them, not confirmed.
    pub heard: Vec<String>,
    /// Going through all of a kind: the list code made.
    pub listed: Option<String>,
    /// The nodes picked, by id.
    pub picked: Vec<String>,
}

/// The ranking as the core takes it: each memory once, most relevant
/// first, named when their words found it (not when it only fills); and
/// each memory's line.
fn ranking(recalled: &Recalled) -> (Vec<Ranked>, HashMap<String, String>) {
    let mut ranked = Vec::new();
    let mut lines = HashMap::new();
    for line in &recalled.named {
        let Some((_, id)) = recalled.ids.iter().find(|(seen, _)| seen == line) else {
            continue;
        };
        if lines.contains_key(id) {
            continue;
        }
        lines.insert(id.clone(), line.clone());
        ranked.push(Ranked {
            id: id.clone(),
            named: !recalled.recent.contains(line),
        });
    }
    (ranked, lines)
}

/// The line of an entry as she reads it: the ranking's own when it found
/// it, else its memory's as recall would write it, else its text; going
/// through all of it, with their words beside it.
struct Lines<'a> {
    shown: &'a Shown,
    ranked: HashMap<String, String>,
    said: &'a HashMap<String, String>,
    thorough: bool,
    zone: Zone,
    here: Option<&'a Here>,
}

impl Lines<'_> {
    fn record(&self, id: &str) -> Option<MemoryRecord> {
        self.shown.memories.get(id).cloned().map(MemoryRecord::from)
    }

    /// The line itself, and with their words beside it when they go there.
    fn of(&self, entry_id: &str, text: &str, at: i64) -> (String, String) {
        let (line, said) = match self.ranked.get(entry_id) {
            Some(line) => (line.clone(), self.said.get(line).cloned()),
            None => match self.record(entry_id) {
                Some(record) => {
                    let line = store::as_known(&record);
                    let line = match self.here {
                        Some(here) => here.theirs(line, Some(&record)),
                        None => line,
                    };
                    (line, store::their_words(&record))
                }
                None => {
                    let day = DateTime::from_timestamp_micros(at)
                        .unwrap_or_default()
                        .with_timezone(&self.zone);
                    (format!("[{}] {}", day.format("%m-%d"), text.trim()), None)
                }
            },
        };
        let shown = match said.filter(|_| self.thorough) {
            Some(said) => myriad_merope::speaking::with_their_words(&line, &said),
            None => line.clone(),
        };
        (line, shown)
    }
}

fn pick_node(shown: &Shown, node: &super::shown::Node) -> PickNode {
    PickNode {
        id: node.id.clone(),
        status: node.status,
        gone: node.gone,
        entries: node
            .facts
            .iter()
            .filter(|entry| shown.ordinary(entry))
            .map(|entry| PickEntry {
                id: entry.id.clone(),
                at: entry.at,
            })
            .collect(),
    }
}

fn find<'a>(shown: &'a Shown, node: &str, id: &str) -> Option<&'a Entry> {
    shown.node(node)?.facts.iter().find(|entry| entry.id == id)
}

/// What comes to mind of `shown` for what `recalled` ranked; going through
/// all of a kind when `cues` say so, at `now`. In a private talk.
pub(crate) fn recollect(
    shown: &Shown,
    own: &Own,
    recalled: &Recalled,
    cues: Option<&Cues>,
    now: DateTime<Utc>,
) -> Recollection {
    recollect_here(shown, own, None, recalled, cues, now)
}

/// [`recollect`]; `here` in a group.
pub(crate) fn recollect_here(
    shown: &Shown,
    own: &Own,
    here: Option<&Here>,
    recalled: &Recalled,
    cues: Option<&Cues>,
    now: DateTime<Utc>,
) -> Recollection {
    let thorough = cues.is_some_and(|cues| cues.thorough);
    let (mut ranked, lines) = ranking(recalled);
    // What is on a node here but not to be taken as a fact never comes,
    // whatever found it: heard comes apart, seen not at all.
    let on_nodes: HashSet<String> = shown
        .nodes
        .iter()
        .filter(|node| !node.gone)
        .flat_map(|node| node.head["entries"].as_array().into_iter().flatten())
        .flat_map(|entry| cited(&entry["refs"]))
        .collect();
    let facts: HashSet<&str> = shown
        .nodes
        .iter()
        .flat_map(|node| node.facts.iter().map(|entry| entry.id.as_str()))
        .collect();
    ranked.retain(|one| !on_nodes.contains(&one.id) || facts.contains(one.id.as_str()));
    let lines = Lines {
        shown,
        ranked: lines,
        said: &recalled.said,
        thorough,
        zone: clock::zone(),
        here,
    };
    let Ok(pick) = recall::pick(PickInput {
        ranked: ranked.clone(),
        nodes: shown
            .nodes
            .iter()
            .map(|node| pick_node(shown, node))
            .collect(),
        about_them: own.about.clone(),
        her: own.her.clone(),
        thorough,
        budget: Default::default(),
    }) else {
        // Ids the core will not take: what was ranked, as before.
        return Recollection {
            named: recalled.named.clone(),
            ..Default::default()
        };
    };
    // What their words brought to mind stays there, as it always did: not
    // said again among what she knows.
    let mut named: Vec<String> = Vec::new();
    let mut push = |(line, shown): (String, String), under: &str| {
        let shown = if under.is_empty() {
            shown
        } else {
            format!("{under}: {shown}")
        };
        if !recalled.brought_to_mind.contains(&line) && !named.contains(&shown) {
            named.push(shown);
        }
    };
    for picked in &pick.picked {
        let name = shown
            .node(&picked.node)
            .and_then(|node| node.name.as_deref())
            .unwrap_or_default();
        for id in &picked.entries {
            if let Some(entry) = find(shown, &picked.node, id) {
                push(lines.of(id, &entry.text, entry.at), name);
            }
        }
    }
    for id in &pick.loose {
        if lines.ranked.contains_key(id) {
            push(lines.of(id, "", 0), "");
        }
    }
    if let Some(about) = &own.about {
        for id in &pick.about_them {
            if let Some(entry) = find(shown, about, id) {
                push(lines.of(id, &entry.text, entry.at), "");
            }
        }
    }
    let heard = own
        .about
        .as_deref()
        .and_then(|about| shown.node(about))
        .map(|node| {
            let mut heard: Vec<&Entry> = node.heard.iter().collect();
            heard.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.id.cmp(&b.id)));
            heard
                .into_iter()
                .take(HEARD)
                .map(|entry| lines.of(&entry.id, &entry.text, entry.at).1)
                .collect()
        })
        .unwrap_or_default();
    let listed = thorough
        .then(|| {
            let kind: Vec<&str> = pick
                .together
                .iter()
                .map(|together| together.node.as_str())
                .collect();
            listed(shown, &kind, cues?, now, &lines)
        })
        .flatten();
    Recollection {
        named,
        heard,
        listed,
        picked: pick
            .picked
            .iter()
            .map(|picked| picked.node.clone())
            .collect(),
    }
}

/// All of a kind, listed by code: every fact on `kind` (the nodes their
/// words found, in the order they rank; never a person's own node, which is
/// no kind of thing), in the stretch of time and the state `cues` ask
/// about, their own words beside each where they add something, and in a
/// group what is about someone else said as theirs. None when she keeps
/// nothing of it, or nothing of a kind was found.
fn listed(
    shown: &Shown,
    kind: &[&str],
    cues: &Cues,
    now: DateTime<Utc>,
    lines: &Lines,
) -> Option<String> {
    if kind.is_empty() {
        return None;
    }
    let zone = super::intent_store::zone_name(now);
    let (from, to) = cues
        .range
        .clone()
        .and_then(|range| {
            listing::range_dates(RangeInput {
                range,
                now: now.timestamp_micros(),
                zone: zone.clone(),
            })
            .ok()
        })
        .map_or((None, None), |dates| (dates.from, dates.to));
    let nodes = kind
        .iter()
        .filter_map(|id| shown.node(id))
        .filter(|node| node.kind.as_deref() != Some("person"))
        .map(|node| ListNode {
            id: node.id.clone(),
            gone: false,
            entries: node
                .facts
                .iter()
                .filter(|entry| shown.ordinary(entry))
                .map(|entry| {
                    let record = entry
                        .memory
                        .as_deref()
                        .and_then(|memory| lines.record(memory));
                    ListEntry {
                        id: entry.row.clone(),
                        at: entry.at,
                        text: match lines.here {
                            Some(here) => here.theirs(entry.text.clone(), record.as_ref()),
                            None => entry.text.clone(),
                        },
                        memory: entry.memory.clone(),
                        quote: record.as_ref().and_then(store::their_words),
                        happened: entry.happened.clone(),
                        state: entry.state.clone(),
                        open: entry.open,
                    }
                })
                .collect(),
        })
        .collect();
    listing::list(ListInput {
        nodes,
        zone,
        from,
        to,
        open: cues.open,
        limit: listing::LIMIT,
    })
    .ok()?
    .section
}
