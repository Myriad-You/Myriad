//! What comes to mind of her nodes as she answers (§4 of the plan): the
//! site ranks what she keeps for their words as it
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
    recall::{self, PickEntry, PickInput, PickNode, PickStanding, Ranked},
    stands,
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
    /// How their life stands, shown every time in a private talk: each
    /// `node name：text`, as the core chose them. None in a group.
    pub resident: Vec<String>,
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
    /// The year it is as she speaks: left out of a day in it.
    year: i32,
    here: Option<&'a Here>,
}

impl Lines<'_> {
    /// The memory row `id`, dated the day she learned it.
    fn record(&self, id: &str) -> Option<MemoryRecord> {
        let mut record = MemoryRecord::from(self.shown.memories.get(id).cloned()?);
        if let Some(learned) = self.shown.learned_of(id)
            && let Some(at) = DateTime::from_timestamp_micros(learned)
        {
            record.created_at = at.fixed_offset();
        }
        Some(record)
    }

    /// The line itself, and with their words beside it when they go there.
    fn of(&self, entry_id: &str, text: &str, at: i64) -> (String, String) {
        let (line, said) = match self.ranked.get(entry_id) {
            Some(line) => (line.clone(), self.said.get(line).cloned()),
            None => match self.record(entry_id) {
                Some(record) => {
                    let line = store::as_known_in(&record, self.year);
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

    /// How one thing on `node` stands now (`node.standing` `item`), with
    /// since when ([`super::since`]): dated as every other line she reads,
    /// by what was said, not by when the row was written.
    fn standing(&self, node: &super::shown::Node, item: &str) -> Option<(String, String)> {
        let standing = node.head.get("standing")?.get(item)?;
        let text = standing.get("value")?.get("text")?.as_str()?.trim();
        if text.is_empty() {
            return None;
        }
        let by_rule = standing.get("author").and_then(|author| author.as_str()) == Some("rule");
        let line = super::since::now(text, &self.since(node, item)?, by_rule);
        Some((line.clone(), line))
    }

    /// What ended on `node` ([`super::since::ended_lines`]): the words of each and its
    /// line, with since when and what stood before it. One resting on
    /// nothing is not said; nor one any version of which rested on a memory
    /// they took back ([`Lines::since`]): the whole line goes, what stood
    /// and what ended it.
    fn ended(&self, node: &super::shown::Node) -> Vec<(String, String)> {
        let since = |item: &str| {
            let rests_on = cited(&node.head["standing"][item]["value"]["because"]);
            if rests_on.is_empty() {
                return None;
            }
            let was = self.shown.stood.get(&(node.id.clone(), item.to_string()));
            Some((self.since(node, item)?, was.cloned()))
        };
        super::since::ended_lines(node, since, super::since::MOST_ENDED)
    }

    /// Since when `item` on `node` stands or ended, as its line says it
    /// ([`super::since::dated`]): told the latest day she learned a memory
    /// it rests on, held or since replaced. None when any of them, or any
    /// an earlier version of it rested on, was taken back
    /// ([`Shown::unsaid_item`]): it is then not said at all, standing or
    /// ended.
    fn since(&self, node: &super::shown::Node, item: &str) -> Option<String> {
        let standing = node.head.get("standing")?.get(item)?;
        let value = standing.get("value")?;
        let because = value.get("because").unwrap_or(&serde_json::Value::Null);
        if self.shown.unsaid_item(&node.id, item, because) {
            return None;
        }
        let learned = cited(because)
            .iter()
            .filter_map(|id| self.shown.told_of(id))
            .max();
        Some(super::since::dated(
            value.get("since"),
            learned,
            standing.get("at")?.as_i64()?,
            &self.zone,
            self.year,
        ))
    }
}

/// The words `items` of `node`'s 「现在」 stand on, trimmed.
fn standing_texts(node: &super::shown::Node, items: &[String]) -> HashSet<String> {
    items
        .iter()
        .filter_map(|item| {
            let text = node
                .head
                .get("standing")?
                .get(item)?
                .get("value")?
                .get("text")?;
            Some(text.as_str()?.trim().to_string())
        })
        .filter(|text| !text.is_empty())
        .collect()
}

/// That more stands on a node than she is shown (`left_out`, the core's
/// count, less those the list of how their life stands shows: `elsewhere`)
/// said plainly, so she knows not all of it is in front of her.
fn more_standing(left_out: usize, elsewhere: usize) -> Option<(String, String)> {
    let left_out = left_out.saturating_sub(elsewhere);
    (left_out > 0).then(|| {
        let line = format!("now: {left_out} more about how things stand here, not shown");
        (line.clone(), line)
    })
}

/// What stands now on `node` as the fold gave it: each item of its
/// `standing`, when it was written, whether the whole of it ended, and the
/// days of the memories its version rests on (what it tells of, from the
/// entry on the node; when she learned it), by which the core says when it
/// is due. A memory not on the node, or not held, has no days.
fn standing_of(shown: &Shown, node: &super::shown::Node) -> Vec<PickStanding> {
    let rests_on = |value: Option<&serde_json::Value>| -> Vec<stands::RestsOn> {
        cited(
            value
                .and_then(|value| value.get("because"))
                .unwrap_or(&serde_json::Value::Null),
        )
        .into_iter()
        .map(|id| {
            let happened = node
                .facts
                .iter()
                .find(|entry| entry.memory.as_deref() == Some(id.as_str()))
                .and_then(|entry| entry.happened.as_ref());
            stands::RestsOn {
                from: happened.map(|happened| happened.from.clone()),
                to: happened.and_then(|happened| happened.to.clone()),
                learned: shown.learned_of(&id).and_then(super::standing::day_of),
            }
        })
        .collect()
    };
    node.head
        .get("standing")
        .and_then(|standing| standing.as_object())
        .map(|standing| {
            standing
                .iter()
                // One resting on what they took back is not hers to say:
                // the core never counts or picks it.
                .filter(|(item, version)| {
                    !shown.unsaid_item(&node.id, item, &version["value"]["because"])
                })
                .filter_map(|(item, version)| {
                    let value = version.get("value");
                    Some(PickStanding {
                        item: item.clone(),
                        at: version.get("at")?.as_i64()?,
                        ended: value
                            .and_then(|value| value.get("ended"))
                            .and_then(|ended| ended.as_bool())
                            .unwrap_or(false),
                        text: value
                            .and_then(|value| value.get("text"))
                            .and_then(|text| text.as_str())
                            .unwrap_or_default()
                            .trim()
                            .to_string(),
                        because: value
                            .and_then(|value| value.get("because"))
                            .and_then(|because| because.as_array())
                            .map(|because| {
                                because
                                    .iter()
                                    .filter_map(|reference| reference.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        rests_on: rests_on(value),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn pick_node(shown: &Shown, node: &super::shown::Node) -> PickNode {
    PickNode {
        id: node.id.clone(),
        name: node.name.clone(),
        status: node.status,
        gone: node.gone,
        entries: node
            .facts
            .iter()
            .filter(|entry| shown.ordinary(entry))
            .map(|entry| PickEntry {
                id: entry.id.clone(),
                at: shown.learned(entry),
            })
            .collect(),
        standing: standing_of(shown, node),
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

/// [`recollect`] with `times` the budget of nodes and entries (and of
/// heard): the memory bench's control that only recalls more.
#[cfg(test)]
pub(crate) fn recollect_times(
    shown: &Shown,
    own: &Own,
    recalled: &Recalled,
    cues: Option<&Cues>,
    now: DateTime<Utc>,
    times: usize,
) -> Recollection {
    let base = recall::Budget::default();
    let budget = recall::Budget {
        picked: base.picked * times,
        entries: base.entries * times,
        about_them: base.about_them * times,
        together: base.together * times,
        ..base
    };
    recollect_within(shown, own, None, recalled, cues, now, budget, HEARD * times)
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
    recollect_within(
        shown,
        own,
        here,
        recalled,
        cues,
        now,
        Default::default(),
        HEARD,
    )
}

#[allow(clippy::too_many_arguments)]
fn recollect_within(
    shown: &Shown,
    own: &Own,
    here: Option<&Here>,
    recalled: &Recalled,
    cues: Option<&Cues>,
    now: DateTime<Utc>,
    budget: recall::Budget,
    heard_most: usize,
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
        year: chrono::Datelike::year(&now.with_timezone(&clock::zone())),
        here,
    };
    let resident_chars = budget.resident_chars;
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
        budget,
        today: super::standing::day_of(now.timestamp_micros()),
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
    // How their life stands, every time: in a private talk only, each
    // under its node's name with since when, as on a picked node. Within
    // the room the core counted its words in: the days beside them are
    // counted here, and one that no longer fits is left out for the next
    // that does, as the core leaves one out.
    let mut room = resident_chars;
    let resident: Vec<(&str, String)> = pick
        .resident
        .iter()
        .filter(|_| here.is_none())
        .filter_map(|one| {
            let node = shown.node(&one.node)?;
            let name = node.name.as_deref().unwrap_or_default().trim();
            let dated = lines.since(node, &one.item)?;
            let line = super::since::resident(name, &one.text, &dated);
            let chars = line.chars().count();
            (chars <= room).then(|| {
                room -= chars;
                (one.node.as_str(), line)
            })
        })
        .collect();
    // How many of a node's stands that list shows: those are not counted
    // again as left out on the node.
    let elsewhere = |node: &str| resident.iter().filter(|(on, _)| *on == node).count();
    for picked in &pick.picked {
        let node = shown.node(&picked.node);
        let name = node
            .and_then(|node| node.name.as_deref())
            .unwrap_or_default();
        // How it stands now comes before what happened on it, and an entry
        // that says the very words it stands on is not said again.
        let standing = node
            .map(|node| standing_texts(node, &picked.standing))
            .unwrap_or_default();
        for item in &picked.standing {
            if let Some(line) = node.and_then(|node| lines.standing(node, item)) {
                push(line, name);
            }
        }
        if let Some(line) = more_standing(picked.standing_left_out, elsewhere(&picked.node)) {
            push(line, name);
        }
        // And what ended on it, before the entries from when it still
        // stood; the fact that ended it is not said again as an entry.
        let ended = node.map(|node| lines.ended(node)).unwrap_or_default();
        for (_, line) in &ended {
            push((line.clone(), line.clone()), name);
        }
        for id in &picked.entries {
            if let Some(entry) = find(shown, &picked.node, id)
                && !standing.contains(entry.text.trim())
                && !ended.iter().any(|(text, _)| text == entry.text.trim())
            {
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
        let standing = shown
            .node(about)
            .map(|node| standing_texts(node, &pick.about_them_standing))
            .unwrap_or_default();
        if let Some(node) = shown.node(about) {
            for item in &pick.about_them_standing {
                if let Some(line) = lines.standing(node, item) {
                    push(line, "");
                }
            }
            if let Some(line) = more_standing(pick.about_them_standing_left_out, elsewhere(about)) {
                push(line, "");
            }
        }
        for id in &pick.about_them {
            if let Some(entry) = find(shown, about, id)
                && !standing.contains(entry.text.trim())
            {
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
            heard.sort_by(|a, b| {
                shown
                    .learned(b)
                    .cmp(&shown.learned(a))
                    .then_with(|| a.id.cmp(&b.id))
            });
            heard
                .into_iter()
                .take(heard_most)
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
    if here.is_none() {
        // Counts only: how many went with the talk, how many the core
        // picked that are not shown, and how many its room left out or cut
        // short.
        tracing::info!(
            shown = resident.len(),
            unread = pick.resident.len() - resident.len(),
            left_out = pick.resident_left_out,
            cut = pick.resident_cut,
            "[memory-nodes] how their life stands went with the private talk"
        );
    }
    let resident = resident.into_iter().map(|(_, line)| line).collect();
    Recollection {
        named,
        heard,
        listed,
        picked: pick
            .picked
            .iter()
            .map(|picked| picked.node.clone())
            .collect(),
        resident,
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
                        at: shown.learned(entry),
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

#[cfg(test)]
mod more_tests {
    /// More standing on a node than she is shown is said, with how many;
    /// nothing is said when all of it is shown.
    #[test]
    fn what_is_not_shown_of_how_things_stand_is_said() {
        assert_eq!(super::more_standing(0, 0), None);
        let (line, shown) = super::more_standing(3, 0).unwrap();
        assert_eq!(line, shown);
        assert!(line.contains("3 more"), "{line}");
        // Those the list of how their life stands shows are not counted.
        assert!(super::more_standing(3, 1).unwrap().0.contains("2 more"));
        assert_eq!(super::more_standing(2, 2), None);
    }
}
