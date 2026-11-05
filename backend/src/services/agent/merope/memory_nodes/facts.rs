//! What she keeps for good of someone put on her nodes: each memory row an
//! entry (`node.entry`, its text the memory's, its refs `memory:<id>`) on
//! the node it is of, through the core's rows (`myriad_nodes::entries`).
//!
//! Where it goes, as she decided it going over a stretch of talk or sorting
//! what she knew before ([`myriad_nodes::entries::Placement`]):
//!
//! - on a node she was shown: that one, or where it went since;
//! - on a topic she starts: one she already keeps under that name or one of
//!   its names is that one; else a topic whose key the core makes from the
//!   audience and the name ([`myriad_nodes::entries::topic_key`]), so the
//!   same name is one node;
//! - about them as a person: their own node (`person:<user_id>`,
//!   「关于你」), started the first time;
//! - nowhere: only the decision is kept.
//!
//! A memory that replaces one goes where the one it replaced went,
//! whatever was decided for it: it says how that same thing is now. A
//! memory placed already is not placed again. The rows and where each
//! memory went are written together ([`super::write::write_with`]), one
//! node at a time, so a node that cannot take its rows leaves the others.
//!
//! Going over a stretch decided against the topics it was shown, on one
//! side or the other while the old topic tables are kept beside the nodes
//! ([`super::switch`]): a topic of the other side is found here by its name.

use std::collections::BTreeMap;

use myriad_nodes::{
    SaidBy,
    entries::{self, FactRowsInput, Placement, Start},
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use super::{
    Author, Error, RowIn, audience,
    known::{self, Known},
    placements::{self, How},
    write::{Beside, write_with},
};
use crate::services::agent::memory::unified::{self, Audience, MemoryKind};
use crate::services::agent::merope::store::GoesUnder;
use crate::services::her_log::pen::Thought;

/// One memory to put on a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fact {
    pub memory: String,
    pub text: String,
    pub said_by: SaidBy,
    pub placement: Placement,
}

/// Where a group of facts goes: a node there already (its key), or one to
/// start.
#[derive(Debug, Clone)]
enum Target {
    Key(String),
    Start(Start),
}

fn random_topic_key() -> String {
    format!(
        "minted:t{}",
        &uuid::Uuid::new_v4().simple().to_string()[..24]
    )
}

pub(super) async fn person_name<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<String, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await?;
    Ok(row
        .and_then(|row| row.try_get::<String>("", "username").ok())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| format!("user {user_id}")))
}

/// The key of the node `id` of the audience, if it is one shown.
fn key_of(known: &[Known], id: &str) -> Option<String> {
    known
        .iter()
        .find(|node| node.id == id)
        .and_then(|node| node.key.clone())
}

/// Where `placement` puts a fact in `user_id`'s `audience`, about `them`;
/// none for nowhere, or a node gone with nothing in its place (left to be
/// placed).
async fn target<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    them: i32,
    known: &[Known],
    placement: &Placement,
) -> Result<Option<(String, Target, How)>, Error> {
    Ok(match placement {
        Placement::Nowhere => None,
        Placement::Node { node } => {
            let Some(node) = known::landing(db, user_id, audience, node).await? else {
                return Ok(None);
            };
            let Some(key) = key_of(known, &node) else {
                return Ok(None);
            };
            Some((node, Target::Key(key), How::Node))
        }
        Placement::Start { name, aliases } => {
            if let Some(node) = known
                .iter()
                .find(|node| node.is_topic() && myriad_merope::topics::goes_by(name, &node.topic()))
                && let Some(key) = &node.key
            {
                return Ok(Some((node.id.clone(), Target::Key(key.clone()), How::Node)));
            }
            let key = entries::topic_key(audience, name)?;
            let id = myriad_nodes::node_id(audience, &key)?;
            // Kept under that key, and gone into another since: there.
            if super::read::node(db, user_id, audience, &id)
                .await?
                .is_some()
            {
                return Ok(match known::landing(db, user_id, audience, &id).await? {
                    Some(node) if node == id => Some((node, Target::Key(key), How::Node)),
                    Some(node) => {
                        key_of(known, &node).map(|key| (node, Target::Key(key), How::Node))
                    }
                    // Forgotten under that name: a topic of its own now.
                    None => {
                        let key = random_topic_key();
                        let id = myriad_nodes::node_id(audience, &key)?;
                        let start = Start {
                            key: Some(key),
                            name: name.clone(),
                            aliases: aliases.clone(),
                        };
                        Some((id, Target::Start(start), How::Node))
                    }
                });
            }
            let start = Start {
                key: None,
                name: name.clone(),
                aliases: aliases.clone(),
            };
            Some((id, Target::Start(start), How::Node))
        }
        Placement::AboutThem => {
            let key = audience::person_key(audience, them);
            let id = myriad_nodes::node_id(audience, &key)?;
            if super::read::node(db, user_id, audience, &id)
                .await?
                .is_some()
            {
                Some((id, Target::Key(key), How::About))
            } else {
                let start = Start {
                    key: Some(key),
                    name: person_name(db, them).await?,
                    aliases: Vec::new(),
                };
                Some((id, Target::Start(start), How::About))
            }
        }
    })
}

/// Puts `facts` on the nodes of `user_id`'s `audience`, as hers; how many
/// were placed. One already placed is left where it is.
pub(crate) async fn put(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    facts: &[Fact],
    pen: &Thought,
) -> Result<usize, Error> {
    put_about(db, user_id, audience, user_id, facts, pen).await
}

/// [`put`], what is only about a person going on `them`'s own node: in a
/// private talk its one person, in a group the member it was about.
pub(crate) async fn put_about(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    them: i32,
    facts: &[Fact],
    pen: &Thought,
) -> Result<usize, Error> {
    let known = known::known(db, user_id, audience).await?;
    let mut nowhere = Beside::default();
    let mut groups: BTreeMap<String, (Target, How, Vec<&Fact>)> = BTreeMap::new();
    for fact in facts {
        if placements::placement(db, &fact.memory).await?.is_some() {
            continue;
        }
        match target(db, user_id, audience, them, &known, &fact.placement).await? {
            Some((node, target, how)) => {
                let group = groups
                    .entry(node)
                    .or_insert_with(|| (target.clone(), how, Vec::new()));
                // A topic started by two facts under one name keeps the
                // other names of both.
                if let (Target::Start(kept), Target::Start(more)) = (&mut group.0, &target) {
                    kept.aliases.extend(more.aliases.iter().cloned());
                }
                group.2.push(fact);
            }
            None if fact.placement == Placement::Nowhere => {
                nowhere.placements.push(placements::Placement {
                    memory_id: fact.memory.clone(),
                    user_id,
                    audience: audience.to_string(),
                    how: How::Nowhere,
                    node_id: None,
                })
            }
            None => {}
        }
    }
    let mut placed = nowhere.placements.len();
    write_with(db, user_id, audience, &[], Some(pen), &nowhere).await?;
    for (node, (target, how, facts)) in groups {
        let (start, key) = match target {
            Target::Key(key) => (None, Some(key)),
            Target::Start(start) => (Some(start), None),
        };
        let made = entries::fact_rows(FactRowsInput {
            audience: audience.to_string(),
            sync: myriad_nodes::Sync::Shared,
            start,
            key,
            facts: facts
                .iter()
                .map(|fact| entries::Fact {
                    memory: fact.memory.clone(),
                    text: fact.text.clone(),
                    said_by: vec![fact.said_by],
                    refs: Vec::new(),
                    revision: String::new(),
                    audience: None,
                })
                .collect(),
        })?;
        let rows: Vec<RowIn> = made
            .rows
            .into_iter()
            .map(|row| RowIn {
                id: row.id,
                node: made.node.clone(),
                kind: row.kind,
                body: row.body,
                author: Author::Her,
            })
            .collect();
        let beside = Beside {
            placements: facts
                .iter()
                .map(|fact| placements::Placement {
                    memory_id: fact.memory.clone(),
                    user_id,
                    audience: audience.to_string(),
                    how,
                    node_id: Some(made.node.clone()),
                })
                .collect(),
            ..Beside::default()
        };
        match write_with(db, user_id, audience, &rows, Some(pen), &beside).await {
            Ok(_) => placed += facts.len(),
            Err(error) => tracing::warn!(%error, node, "[memory-nodes] facts not put on a node"),
        }
    }
    Ok(placed)
}

/// Who said a memory, as the core tags it in a private talk.
pub(crate) fn said_by(speaker: &str) -> SaidBy {
    if speaker == "agent" {
        SaidBy::Her
    } else {
        SaidBy::Partner
    }
}

/// Who said a memory, as the core tags it in `audience`: in a group,
/// everyone but her is someone there.
fn said_by_in(audience: &str, speaker: &str) -> SaidBy {
    let who = if speaker == "agent" {
        audience::Speaker::Her
    } else {
        audience::Speaker::Person
    };
    audience::said_by(audience, who).unwrap_or_else(|| said_by(speaker))
}

/// Where the memory a memory replaced was put, if it was.
async fn replaced<C: ConnectionTrait>(db: &C, memory: &str) -> Result<Option<Placement>, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT p.how, p.node_id FROM agent_memories m \
             JOIN agent_node_placements p ON p.memory_id = m.id \
             WHERE m.superseded_by = $1 ORDER BY p.decided_at DESC LIMIT 1",
            [memory.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let how: String = row.try_get("", "how")?;
    let node: Option<String> = row.try_get("", "node_id")?;
    Ok(match (how.as_str(), node) {
        ("about", _) => Some(Placement::AboutThem),
        ("node", Some(node)) => Some(Placement::Node { node }),
        _ => Some(Placement::Nowhere),
    })
}

/// A topic of the old tables, by its id, as a topic to start under its
/// names (found by them on the nodes).
async fn old_topic<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    id: &str,
) -> Result<Option<Placement>, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT name, aliases FROM agent_topic_nodes WHERE user_id = $1 AND id = $2",
            [user_id.into(), id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let aliases: serde_json::Value = row.try_get("", "aliases")?;
    Ok(Some(Placement::Start {
        name: row.try_get("", "name")?,
        aliases: serde_json::from_value(aliases).unwrap_or_default(),
    }))
}

/// Where going over a stretch put a fact, on the nodes' side.
async fn from_settled<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    goes: &GoesUnder,
) -> Result<Option<Placement>, Error> {
    Ok(match goes {
        GoesUnder::Nowhere => Some(Placement::Nowhere),
        GoesUnder::AboutThem => Some(Placement::AboutThem),
        GoesUnder::New { name, aliases } => Some(Placement::Start {
            name: name.clone(),
            aliases: aliases.clone(),
        }),
        GoesUnder::Topic(id) => old_topic(db, user_id, id).await?,
    })
}

/// What going over a stretch of `user_id`'s talk in front of `present`
/// kept for good (`updates`, already written), put on her nodes: their
/// private talk's, or the group's (kept with the site owner), what is only
/// about them on their own node there; and in a group, what they said
/// about themselves on 「关于你」 in their own private talk too
/// ([`super::group_facts`]). Only while nodes are written and her mind is
/// here. What a private talk does not place now is sorted later
/// ([`super::sorting`]).
pub(crate) async fn settled(
    db: &DatabaseConnection,
    user_id: i32,
    site_owner: Option<i32>,
    present: &Audience,
    updates: &crate::services::agent::merope::chat_remember::ChatMemoryUpdates,
) {
    if !super::mode().writes() || user_id <= 0 {
        return;
    }
    let Some(home) = audience::of_talk(present, user_id, site_owner) else {
        return;
    };
    let Some(pen) = crate::services::her_log::pen::think() else {
        tracing::info!(
            "[memory-nodes] what a stretch kept not put on nodes: her mind is elsewhere"
        );
        return;
    };
    let audience = home.audience;
    let result = async {
        let rows = unified::active_in(db, user_id, present, &MemoryKind::ABOUT_PERSON).await?;
        let mut facts: Vec<Fact> = Vec::new();
        for update in &updates.updates {
            let Some(fact) = &update.fact else {
                continue;
            };
            let Some(holder) = rows.iter().find(|row| {
                row.source == "chat"
                    && &crate::services::agent::merope::ingest::compact_summary(&row.content)
                        == fact
            }) else {
                continue;
            };
            if facts.iter().any(|kept| kept.memory == holder.id) {
                continue;
            }
            let placement = match replaced(db, &holder.id).await? {
                Some(placement) => Some(placement),
                None => match &update.topic {
                    Some(goes) => from_settled(db, user_id, goes).await?,
                    None => None,
                },
            };
            if let Some(placement) = placement {
                facts.push(Fact {
                    memory: holder.id.clone(),
                    text: holder.content.clone(),
                    said_by: said_by_in(&audience, &holder.speaker),
                    placement,
                });
            }
        }
        let placed = put_about(db, home.user_id, &audience, user_id, &facts, &pen).await?;
        if present.is_group() {
            super::group_facts::to_their_talk(db, user_id, site_owner, &audience, &facts, &pen)
                .await;
        }
        Ok::<usize, Error>(placed)
    }
    .await;
    match result {
        Ok(placed) if placed > 0 => {
            tracing::info!(placed, "[memory-nodes] what a stretch kept is on her nodes")
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "[memory-nodes] what a stretch kept not put on nodes"),
    }
}

/// How one of her nodes is named among the topics going over a stretch is
/// shown: its id after this, so its label reads back as the node.
pub(crate) const SHOWN: &str = "node:";

/// The topics going over a stretch of `user_id`'s private talk shows: those
/// it was going to show (`old`, the old topic tables'), or her nodes'
/// topics instead once she reads her nodes, and either way first the nodes
/// placing the stretch just used (`used`). A node shown is named
/// [`SHOWN`]`<id>`; one of the same name as an old topic is not shown twice.
pub(crate) async fn shown_topics(
    db: &DatabaseConnection,
    user_id: i32,
    site_owner: Option<i32>,
    talk: &str,
    used: &[String],
    old: Vec<crate::services::agent::merope::store::OfferedTopic>,
) -> Vec<crate::services::agent::merope::store::OfferedTopic> {
    use crate::services::agent::merope::store::OfferedTopic;
    let mode = super::mode();
    if !mode.writes() || user_id <= 0 {
        return old;
    }
    let audience = audience::private_talk(user_id, site_owner);
    let known = match known::known(db, user_id, &audience).await {
        Ok(known) => known,
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] her nodes could not be shown");
            return old;
        }
    };
    let as_shown = |node: &Known| OfferedTopic {
        id: format!("{SHOWN}{}", node.id),
        name: node.name.clone(),
        aliases: node.aliases.clone(),
        holds: node.holds.clone(),
    };
    let mut shown: Vec<OfferedTopic> = used
        .iter()
        .filter_map(|id| {
            known
                .iter()
                .find(|node| &node.id == id && !node.is_person())
        })
        .map(as_shown)
        .collect();
    let rest: Vec<OfferedTopic> = if mode.reads() {
        let topics: Vec<&Known> = known.iter().filter(|node| node.is_topic()).collect();
        let seen: Vec<myriad_merope::topics::Topic> =
            topics.iter().map(|node| node.topic()).collect();
        myriad_merope::topics::offered(talk, &seen)
            .into_iter()
            .map(|at| as_shown(topics[at]))
            .collect()
    } else {
        old
    };
    for topic in rest {
        let again = shown.iter().any(|kept| {
            kept.id == topic.id || myriad_merope::topics::same_name(&kept.name, &topic.name)
        });
        if !again {
            shown.push(topic);
        }
    }
    let at_most = myriad_merope::topics::OFFERED.max(used.len());
    shown.truncate(at_most);
    shown
}
