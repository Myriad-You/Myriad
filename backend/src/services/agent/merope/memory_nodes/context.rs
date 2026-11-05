//! What she is shown of her nodes every time she speaks in a private talk,
//! while she reads them (§4 of the plan, the native host's `NodeRecall`):
//! read once a turn ([`for_turn`]), laid out by the sections that show it.
//!
//! - **The person** (「关于你」 in their talk): what they are to her and how
//!   she comes across (her views `us` and `lands`), what still stings
//!   (what is not wrapped up), the bits only the two of them share, and
//!   how she sums them up when her upkeep has.
//! - **Her** (「她自己」 in their talk): what they told her about herself.
//! - **What is going on**: one or two things in progress with them, besides
//!   those their words brought.
//! - **What she meant to bring up and may now**: an intent come due, and
//!   one of hers that waits for nothing (her next chance); never their
//!   instruction to do something, which is done or raised when it comes due
//!   (`intents_due`).
//!
//! Nothing of a group or another account is read for a private talk, with
//! one exception the site has always kept: what still stings that was born
//! in a group with this person is felt in their private talk too. It is
//! read as such, alone: their own node in each group's audience
//! (`person:<id>`), its open items only ([`sores_in_groups`]). Her own
//! audience (`own:`) is not read here; her own life comes where it always
//! did.
//!
//! A group turn is read the same way, from that group's audience alone
//! ([`super::group_context`]).
//!
//! Each section is worded as the one it replaces (`myriad_merope::speaking`
//! and the modules of each source), from the memory row each item names:
//! the node decides what she is shown, the row is still where its time and
//! detail are kept.

use std::collections::{BTreeSet, HashMap};

use chrono::{DateTime, Utc};
use myriad_nodes::{
    body::{Action, IntentStatus, Origin},
    intents::{self, DueInput},
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
    Statement,
};
use serde_json::Value;

use super::{
    Error, audience,
    group_context::Group,
    hanging, heads,
    intent_store::{self, Held},
    known,
    recall::Own,
    shown::{self, Shown, cited},
};
use crate::models::entities::agent_memories;
use crate::services::agent::memory::unified::Audience;
use crate::services::agent::merope::{bits, sore};

/// What she may bring up now, at most (the native budget).
pub(crate) const INTENTS: usize = 2;
/// Things in progress with them shown, at most.
pub(crate) const ONGOING: usize = 2;
/// What they told her about herself shown, newest first.
const TOLD_SHOWN: usize = 3;

/// A talk as she reads it this turn: a private talk, or a group
/// ([`super::group_context`]).
#[derive(Debug, Clone)]
pub(crate) struct NodeTurn {
    pub own: Own,
    pub shown: Shown,
    /// What still stings with them, oldest first: in private wherever it
    /// was born, in a group what they did there.
    pub sores: Vec<sore::Sore>,
    /// What she may bring up now, oldest first; none in a group.
    pub intents: Vec<Held>,
    /// The group, in one.
    pub group: Option<Group>,
}

/// This turn's read of `user_id`'s talk in front of `present` while she
/// reads her nodes; none otherwise, or when it cannot be read (then she
/// reads as before).
pub(crate) async fn for_turn(
    db: &DatabaseConnection,
    user_id: i32,
    present: &Audience,
) -> Option<NodeTurn> {
    if !super::mode().reads() || user_id <= 0 {
        return None;
    }
    let read = match present.group_id() {
        None => load(db, user_id, crate::services::agent::merope::clock::now())
            .await
            .map(Some),
        Some(venue) => super::group_context::load(db, user_id, venue).await,
    };
    match read {
        Ok(turn) => turn,
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] her nodes could not be read for a turn");
            None
        }
    }
}

/// [`for_turn`] at `now`, read whatever the switch.
pub(crate) async fn load(
    db: &DatabaseConnection,
    user_id: i32,
    now: DateTime<Utc>,
) -> Result<NodeTurn, Error> {
    let site_owner = hanging::site_owner(db).await;
    let talk = audience::private_talk(user_id, site_owner);
    let about = myriad_nodes::node_id(&talk, &audience::person_key(&talk, user_id))?;
    let her = myriad_nodes::node_id(&talk, "her")?;
    let shown = shown::shown(db, user_id, &talk, Some(&about)).await?;
    let mut sores: Vec<sore::Sore> = shown
        .node(&about)
        .map(|node| open_sores(&node.head, &shown.memories))
        .unwrap_or_default();
    if let Some(owner) = site_owner {
        sores.extend(sores_in_groups(db, user_id, owner).await?);
    }
    sores.sort_by_key(|sore| sore.since);
    let intents = may_now(db, user_id, &talk, now).await?;
    Ok(NodeTurn {
        own: Own {
            about: Some(about),
            her: Some(her),
        },
        shown,
        sores,
        intents,
        group: None,
    })
}

/// The open items of `head` not let go of that are sore spots, as their
/// rows keep them.
pub(super) fn open_sores(
    head: &Value,
    memories: &HashMap<String, agent_memories::Model>,
) -> Vec<sore::Sore> {
    head["open"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, item)| item["value"]["closed"].as_bool() != Some(true))
        .filter_map(|(_, item)| {
            cited(&item["value"]["because"])
                .iter()
                .find_map(|id| memories.get(id))
                .filter(|row| row.source == sore::SOURCE)
                .and_then(sore::sore_of)
        })
        .collect()
}

/// What still stings that was born with `user_id` in a group: their own
/// node in each group's audience (kept with the site owner `owner`), its
/// open items alone. The one read of a group for a private talk.
pub(crate) async fn sores_in_groups<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    owner: i32,
) -> Result<Vec<sore::Sore>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT status, node FROM agent_node_heads \
             WHERE user_id = $1 AND audience LIKE 'group:%' AND node_key = $2 \
             ORDER BY audience",
            [owner.into(), format!("person:{user_id}").into()],
        ))
        .await?;
    let mut heads = Vec::new();
    for row in &rows {
        if known::is_live(&row.try_get::<String>("", "status")?) {
            heads.push(row.try_get::<Value>("", "node")?);
        }
    }
    let named: Vec<String> = heads
        .iter()
        .flat_map(|head| head["open"].as_object().into_iter().flatten())
        .flat_map(|(_, item)| cited(&item["value"]["because"]))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let held = known::held(db, named).await?;
    let memories: HashMap<String, agent_memories::Model> = if held.is_empty() {
        HashMap::new()
    } else {
        agent_memories::Entity::find()
            .filter(agent_memories::Column::Id.is_in(held))
            .all(db)
            .await?
            .into_iter()
            // Theirs, and born in a group.
            .filter(|row| row.user_id == Some(user_id) && row.venue.starts_with("group:"))
            .map(|row| (row.id.clone(), row))
            .collect()
    };
    Ok(heads
        .iter()
        .flat_map(|head| open_sores(head, &memories))
        .collect())
}

/// What she may bring up in `audience` now: her intents waiting to be said
/// (never an instruction to do something) that are due by the core, or
/// hers with nothing to wait for; oldest first, at most [`INTENTS`].
async fn may_now(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    now: DateTime<Utc>,
) -> Result<Vec<Held>, Error> {
    let waiting: Vec<Held> = intent_store::held(db, user_id, audience)
        .await?
        .into_iter()
        .filter(|held| held.waiting() && held.action != Some(Action::Work))
        .collect();
    let due: BTreeSet<(String, String)> = if waiting.iter().any(|held| held.trigger.is_some()) {
        let rows = heads::rows_of(db, user_id, audience).await?;
        intents::due(DueInput {
            bodies: heads::bodies(&rows)?,
            rows: rows.into_iter().map(|kept| kept.row).collect(),
            ledger: Vec::new(),
            now: now.timestamp_micros(),
        })?
        .due
        .into_iter()
        .map(|due| (due.node, due.intent))
        .collect()
    } else {
        BTreeSet::new()
    };
    Ok(choose_now(waiting, &due))
}

/// Of `waiting`, those she may bring up now: due (`due`, by node and
/// intent), or hers with nothing to wait for; oldest first, at most
/// [`INTENTS`].
pub(crate) fn choose_now(waiting: Vec<Held>, due: &BTreeSet<(String, String)>) -> Vec<Held> {
    let mut now: Vec<Held> = waiting
        .into_iter()
        .filter(|held| held.status == IntentStatus::Waiting && held.action != Some(Action::Work))
        .filter(|held| {
            due.contains(&(held.node.clone(), held.id.clone()))
                || (held.origin == Some(Origin::Her) && held.trigger.is_none())
        })
        .collect();
    now.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
    now.truncate(INTENTS);
    now
}

impl NodeTurn {
    fn about(&self) -> Option<&shown::Node> {
        self.shown.node(self.own.about.as_deref()?)
    }

    fn her(&self) -> Option<&shown::Node> {
        self.shown.node(self.own.her.as_deref()?)
    }

    fn view(&self, view: &str) -> Option<&Value> {
        self.about()?.head["views"].get(view)
    }

    /// What they told her about herself, newest first.
    pub(crate) fn told_section(&self, now: DateTime<Utc>) -> Option<String> {
        if self.group.is_some() {
            return None;
        }
        let her = self.her()?;
        let mut told: Vec<(String, DateTime<Utc>)> = her
            .facts
            .iter()
            .map(|entry| {
                match entry
                    .memory
                    .as_ref()
                    .and_then(|id| self.shown.memories.get(id))
                {
                    Some(row) => (row.content.clone(), row.created_at.with_timezone(&Utc)),
                    None => (
                        entry.text.clone(),
                        DateTime::from_timestamp_micros(entry.at).unwrap_or_default(),
                    ),
                }
            })
            .collect();
        told.sort_by(|a, b| b.1.cmp(&a.1));
        let told: Vec<(String, String)> = told
            .into_iter()
            .take(TOLD_SHOWN)
            .map(|(what, at)| (what, myriad_merope::doing::ago_text(now - at)))
            .collect();
        myriad_merope::making_sense::told_section_titled(
            &told,
            "What they have told you about yourself",
        )
    }

    /// What only the two of them share, freshest first, at most `limit`.
    pub(crate) fn bits_section(&self, limit: usize) -> Option<String> {
        if self.group.is_some() {
            return None;
        }
        let about = self.about()?;
        let mut rows: Vec<&agent_memories::Model> = about
            .facts
            .iter()
            .filter_map(|entry| self.shown.memories.get(entry.memory.as_ref()?))
            .filter(|row| row.source == "bit")
            .collect();
        rows.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        rows.dedup_by(|a, b| a.id == b.id);
        let bits: Vec<(String, String)> = rows
            .into_iter()
            .filter_map(bits::bit_of)
            .take(limit)
            .collect();
        myriad_merope::speaking::format_bits_section(&bits, false)
    }

    /// How she comes across with them, as she last found it.
    pub(crate) fn lands_section(&self, now: DateTime<Utc>) -> Option<String> {
        if self.group.is_some() {
            return self.group_lands_section(now);
        }
        let row = self.shown.memory_of(self.view("lands")?)?;
        myriad_merope::speaking::format_lands_section(
            &row.content,
            row.created_at.with_timezone(&Utc),
            now,
            false,
        )
    }

    /// What they are to her, as she last put it, and before.
    pub(crate) fn us_section(&self, now: DateTime<Utc>) -> Option<String> {
        if self.group.is_some() {
            return None;
        }
        let us = bits::us_of(self.shown.memory_of(self.view("us")?)?.clone());
        myriad_merope::speaking::format_us_section(
            &us.now,
            us.since.with_timezone(&Utc),
            us.before.as_deref(),
            us.first
                .as_ref()
                .map(|(first, at)| (first.as_str(), at.with_timezone(&Utc))),
            now,
        )
    }

    /// How she sums them up and what is not wrapped up with them, beyond
    /// what the sections above say: what her upkeep wrote on 「关于你」.
    pub(crate) fn about_head_section(&self) -> Option<String> {
        if self.group.is_some() {
            return self.group_head_section();
        }
        let lines = head_lines(self.about()?, &self.shown);
        section(
            "What you make of them",
            "How you sum them up, and what is not wrapped up with them, as you last went over what you keep: yours to go by, never to recite.",
            "their_node",
            &lines,
        )
    }

    /// The same of what they told her about herself, on 「她自己」.
    pub(crate) fn her_head_section(&self) -> Option<String> {
        if self.group.is_some() {
            return None;
        }
        let lines = head_lines(self.her()?, &self.shown);
        section(
            "What you make of yourself with them",
            "How you sum up what they told you about yourself, as you last went over it: yours to go by, never to recite.",
            "her_node",
            &lines,
        )
    }

    /// One or two things going on with them now, besides `picked`: the
    /// most recently added to.
    pub(crate) fn ongoing_section(&self, picked: &[String]) -> Option<String> {
        if self.group.is_some() {
            return None;
        }
        use myriad_nodes::body::Status;
        let mut going: Vec<&shown::Node> = self
            .shown
            .nodes
            .iter()
            .filter(|node| !node.gone && node.kind.as_deref() == Some("task"))
            .filter(|node| matches!(node.status, None | Some(Status::Active | Status::Ongoing)))
            .filter(|node| !picked.contains(&node.id))
            .collect();
        going.sort_by(|a, b| b.latest.cmp(&a.latest).then_with(|| a.id.cmp(&b.id)));
        let lines: Vec<String> = going
            .into_iter()
            .take(ONGOING)
            .filter_map(|node| {
                let name = node.name.as_deref()?.trim();
                let about = summary_of(&node.head).into_iter().next().or_else(|| {
                    node.facts
                        .iter()
                        .max_by_key(|entry| entry.at)
                        .map(|entry| entry.text.clone())
                });
                Some(match about {
                    Some(about) => format!("- {name}: {}", about.trim()),
                    None => format!("- {name}"),
                })
            })
            .collect();
        (!lines.is_empty()).then(|| {
            format!(
                "## Going on now\nWhat is going on with them now, as you keep it:\n{}",
                myriad_agent_rules::untrusted_block("going_on", &lines.join("\n"))
            )
        })
    }

    /// What she meant to bring up with them and may now.
    pub(crate) fn intents_section(&self) -> Option<String> {
        let lines: Vec<String> = self
            .intents
            .iter()
            .filter(|held| !held.text.trim().is_empty())
            .map(|held| match held.origin {
                Some(Origin::Partner) => format!("- {} (they asked you to)", held.text.trim()),
                _ => format!("- {}", held.text.trim()),
            })
            .collect();
        (!lines.is_empty()).then(|| {
            format!(
                "## What you meant to bring up\nWhat you meant to bring up with them, and may now. Bring one in when the talk leaves room for it, in your own words: not all at once, never as a list.\n{}",
                myriad_agent_rules::untrusted_block("meant_to", &lines.join("\n"))
            )
        })
    }
}

/// What others told her about them that they never confirmed: never a fact.
pub(crate) fn heard_section(heard: &[String]) -> Option<String> {
    let lines: Vec<String> = heard
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(|line| format!("- {line}"))
        .collect();
    (!lines.is_empty()).then(|| {
        format!(
            "## Heard about them\nWhat others told you about them that they have not confirmed: not a fact. Never say it as one; if it comes up, you may ask them once.\n{}",
            myriad_agent_rules::untrusted_block("heard", &lines.join("\n"))
        )
    })
}

pub(super) fn section(title: &str, about: &str, tag: &str, lines: &[String]) -> Option<String> {
    (!lines.is_empty()).then(|| {
        format!(
            "## {title}\n{about}\n{}",
            myriad_agent_rules::untrusted_block(tag, &lines.join("\n"))
        )
    })
}

/// A head's summary, sentence by sentence.
fn summary_of(head: &Value) -> Vec<String> {
    head["summary"]["value"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sentence| sentence["text"].as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .collect()
}

/// What a person's head says beyond its own sections: its summary, her
/// views but `us` and `lands`, what is not wrapped up that is no sore spot.
pub(super) fn head_lines(node: &shown::Node, shown: &Shown) -> Vec<String> {
    let mut lines: Vec<String> = summary_of(&node.head)
        .into_iter()
        .map(|text| format!("- {text}"))
        .collect();
    for (id, item) in node.head["views"].as_object().into_iter().flatten() {
        if id == "us" || id == "lands" {
            continue;
        }
        if let Some(text) = item["value"]["text"].as_str().map(str::trim)
            && !text.is_empty()
        {
            lines.push(format!("- {text}"));
        }
    }
    for (_, item) in node.head["open"].as_object().into_iter().flatten() {
        let named = cited(&item["value"]["because"]);
        let a_sore = named.iter().any(|id| {
            shown
                .memories
                .get(id)
                .is_some_and(|row| row.source == sore::SOURCE)
        });
        // One whose sore spot went (faded, let go) is not here either.
        let gone = !named.is_empty() && shown.memory_of(item).is_none();
        if a_sore || gone || item["value"]["closed"].as_bool() == Some(true) {
            continue;
        }
        if let Some(text) = item["value"]["text"].as_str().map(str::trim)
            && !text.is_empty()
        {
            lines.push(format!("- Not wrapped up: {text}"));
        }
    }
    lines
}
