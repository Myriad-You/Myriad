//! Her old topics (`agent_topic_nodes`) and what she put under them
//! (`agent_memory_topics`, `migrations/topic_nodes.sql`) moved onto her
//! nodes ([`super::migrate`]). No model is asked.
//!
//! - **A topic** is a topic node under its old key (`minted:<token>`): in
//!   the site owner's talk (`personal:local-owner`, where the old ones were
//!   all made) its id is the old topic's id; any other account's moves to
//!   `personal:<user_id>`. A topic her nodes already have under another key
//!   that goes by its name (started there by going over a stretch while
//!   the switch wrote, `facts::from_settled`) is that node, not a second
//!   one. A node of its key forgotten or taken back since is not written
//!   to; one gone into another is written where it went.
//! - **What was under it**, of a memory still held and not placed on a node
//!   yet, is an entry on that node (`entries::fact_rows`): the memory's
//!   text, `memory:<id>`, and the lines of theirs it quotes
//!   (`message:<id>`, found as forgetting a memory finds them,
//!   `memory::origins`), with no time or state (not asked again); and is
//!   kept placed there. One topic is one write, under the audience's lock.
//! - A placement under no topic (looked at, not hung) is not moved here:
//!   it is sorted again ([`super::migrate_replay`]).

use std::collections::BTreeMap;

use myriad_nodes::{
    entries::{self, FactRowsInput, Start},
    key::Key,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::Value;

use super::{
    Author, Error, RowIn, audience, facts, known,
    migrate::{Counts, Halt, her_pen},
    placements::{self, How},
    read,
    write::{Beside, write_with},
};
use crate::services::agent::memory::origins;

/// Lines found for one memory, at most.
const LINES: u64 = 4;

/// Where each old topic went: by account and old topic id, the audience
/// and node.
pub(crate) type Moved = BTreeMap<(i32, String), (String, String)>;

struct Old {
    id: String,
    key: String,
    name: String,
    aliases: Vec<String>,
}

/// One old placement to move.
struct Under {
    memory: String,
    text: String,
    speaker: String,
    source: String,
    evidence: Option<String>,
    at: chrono::DateTime<chrono::FixedOffset>,
}

async fn accounts<C: ConnectionTrait>(db: &C) -> Result<Vec<i32>, Error> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT user_id FROM agent_topic_nodes WHERE user_id > 0 ORDER BY user_id",
        ))
        .await?;
    rows.iter()
        .map(|row| Ok(row.try_get::<i32>("", "user_id")?))
        .collect()
}

async fn topics<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<Vec<Old>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, node_key, name, aliases FROM agent_topic_nodes \
             WHERE user_id = $1 ORDER BY created_at, id",
            [user_id.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            let aliases: Value = row.try_get("", "aliases")?;
            Ok(Old {
                id: row.try_get("", "id")?,
                key: row.try_get("", "node_key")?,
                name: row.try_get("", "name")?,
                aliases: serde_json::from_value(aliases).unwrap_or_default(),
            })
        })
        .collect()
}

/// What `user_id` put under the topic `topic`, of memories still held and
/// on no node yet, in the order it was put.
async fn under<C: ConnectionTrait>(db: &C, user_id: i32, topic: &str) -> Result<Vec<Under>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.content, m.speaker, m.source, m.evidence, m.created_at \
             FROM agent_memory_topics p JOIN agent_memories m ON m.id = p.memory_id \
             WHERE p.user_id = $1 AND p.topic_id = $2 \
               AND m.invalid_at IS NULL AND m.superseded_by IS NULL \
               AND NOT EXISTS (SELECT 1 FROM agent_node_placements n WHERE n.memory_id = m.id) \
             ORDER BY p.linked_at, m.id",
            [user_id.into(), topic.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok(Under {
                memory: row.try_get("", "id")?,
                text: row.try_get("", "content")?,
                speaker: row.try_get("", "speaker")?,
                source: row.try_get("", "source")?,
                evidence: row.try_get("", "evidence")?,
                at: row.try_get("", "created_at")?,
            })
        })
        .collect()
}

/// Old placements under a topic of memories still held: all, and those
/// on a node already.
async fn placements_held<C: ConnectionTrait>(db: &C) -> Result<(usize, usize), Error> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS n, COUNT(n.memory_id) AS placed \
             FROM agent_memory_topics p JOIN agent_memories m ON m.id = p.memory_id \
             LEFT JOIN agent_node_placements n ON n.memory_id = m.id \
             WHERE p.topic_id IS NOT NULL AND m.invalid_at IS NULL AND m.superseded_by IS NULL",
        ))
        .await?;
    let Some(row) = row else {
        return Ok((0, 0));
    };
    Ok((
        row.try_get::<i64>("", "n")?.max(0) as usize,
        row.try_get::<i64>("", "placed")?.max(0) as usize,
    ))
}

/// What a dry run counts: the old topics and what is under them.
pub(crate) async fn count<C: ConnectionTrait>(db: &C, counts: &mut Counts) -> Result<(), Error> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS n FROM agent_topic_nodes",
        ))
        .await?;
    counts.topics = row
        .map(|row| row.try_get::<i64>("", "n"))
        .transpose()?
        .unwrap_or(0)
        .max(0) as usize;
    (counts.placements, counts.placed_before) = placements_held(db).await?;
    Ok(())
}

/// The lines of `user_id`'s that a memory written at `at` quotes in their
/// private talk, the way forgetting it finds them (`memory::origins`).
async fn lines<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    under: &Under,
) -> Result<Vec<String>, Error> {
    if under.source != "chat" {
        return Ok(Vec::new());
    }
    let Some(quote) = origins::quote(under.evidence.as_deref()) else {
        return Ok(Vec::new());
    };
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' \
               AND (s.context->>'venue') IS NULL AND m.role = 'user' \
               AND m.created_at BETWEEN $3 AND $4 \
               AND position($2 in regexp_replace(m.content, '\\s+', ' ', 'g')) > 0 \
             ORDER BY m.created_at, m.id LIMIT $5",
            [
                user_id.into(),
                quote.into(),
                (under.at - origins::QUOTED_WITHIN).into(),
                under.at.into(),
                (LINES as i64).into(),
            ],
        ))
        .await?;
    rows.iter()
        .map(|row| Ok(format!("message:{}", row.try_get::<i32>("", "id")?)))
        .collect()
}

/// How an old topic goes onto `user_id`'s `audience`.
enum Way {
    /// Its node there already (its old key, or where it went): the key.
    Kept(String),
    /// A node there that goes by its name.
    Joined(String),
    Start(Start),
    /// Its node forgotten or taken back since.
    Gone,
    /// A key or name the core does not read.
    Bad,
}

async fn way<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    old: &Old,
) -> Result<(Option<String>, Way), Error> {
    let Ok(Key::Minted(_)) = Key::parse(&old.key) else {
        return Ok((None, Way::Bad));
    };
    let id = myriad_nodes::node_id(audience, &old.key)?;
    if read::node(db, user_id, audience, &id).await?.is_some() {
        let Some(landed) = known::landing(db, user_id, audience, &id).await? else {
            return Ok((None, Way::Gone));
        };
        let head = read::node(db, user_id, audience, &landed).await?;
        let key = head
            .as_ref()
            .and_then(|head| head["key"].as_str())
            .map(str::to_string);
        return Ok(match key {
            Some(key) => (Some(landed), Way::Kept(key)),
            None => (None, Way::Gone),
        });
    }
    let shown = known::known(db, user_id, audience).await?;
    if let Some(node) = shown
        .iter()
        .find(|node| node.is_topic() && myriad_merope::topics::goes_by(&old.name, &node.topic()))
        && let Some(key) = &node.key
    {
        return Ok((Some(node.id.clone()), Way::Joined(key.clone())));
    }
    Ok((
        Some(id),
        Way::Start(Start {
            key: Some(old.key.clone()),
            name: old.name.clone(),
            aliases: old.aliases.clone(),
        }),
    ))
}

/// Moves every old topic and what is under it ([module docs](self)).
pub(crate) async fn run(
    db: &DatabaseConnection,
    site_owner: Option<i32>,
    counts: &mut Counts,
) -> Result<Moved, Halt> {
    let mut moved = Moved::new();
    (counts.placements, counts.placed_before) = placements_held(db).await?;
    for user_id in accounts(db).await? {
        let talk = audience::private_talk(user_id, site_owner);
        for old in topics(db, user_id).await? {
            counts.topics += 1;
            let (node, way) = way(db, user_id, &talk, &old).await?;
            let (start, key) = match way {
                Way::Gone => {
                    counts.topics_gone += 1;
                    continue;
                }
                Way::Bad => {
                    counts.topics_bad += 1;
                    continue;
                }
                Way::Kept(key) => {
                    counts.topics_kept += 1;
                    (None, Some(key))
                }
                Way::Joined(key) => {
                    counts.topics_joined += 1;
                    (None, Some(key))
                }
                Way::Start(start) => (Some(start), None),
            };
            let Some(node) = node else { continue };
            moved.insert((user_id, old.id.clone()), (talk.clone(), node.clone()));
            let mut facts = Vec::new();
            for under in under(db, user_id, &old.id).await? {
                if super::homes::clean(&under.text).is_empty() {
                    counts.empty += 1;
                    continue;
                }
                let refs = lines(db, user_id, &under).await?;
                counts.with_message += usize::from(!refs.is_empty());
                facts.push(entries::Fact {
                    memory: under.memory,
                    text: under.text,
                    said_by: vec![facts::said_by(&under.speaker)],
                    refs,
                    revision: String::new(),
                    audience: None,
                    happened: None,
                    state: None,
                    open: None,
                });
            }
            if start.is_none() && facts.is_empty() {
                continue;
            }
            let starts = start.is_some();
            // A name or a text the core cannot keep: the topic is left.
            let Ok(made) = entries::fact_rows(FactRowsInput {
                audience: talk.clone(),
                sync: myriad_nodes::Sync::Shared,
                start,
                key,
                facts,
            }) else {
                counts.topics_bad += 1;
                moved.remove(&(user_id, old.id.clone()));
                continue;
            };
            let beside = Beside {
                placements: made
                    .rows
                    .iter()
                    .filter(|row| row.kind == "node.entry")
                    .filter_map(|row| entry_memory(&row.body))
                    .map(|memory| placements::Placement {
                        memory_id: memory,
                        user_id,
                        audience: talk.clone(),
                        how: How::Node,
                        node_id: Some(made.node.clone()),
                    })
                    .collect(),
                ..Beside::default()
            };
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
            let pen = her_pen()?;
            write_with(db, user_id, &talk, &rows, Some(&pen), &beside).await?;
            counts.topics_started += usize::from(starts);
            counts.placed += beside.placements.len();
        }
    }
    Ok(moved)
}

/// The memory an entry's body names.
fn entry_memory(body: &Value) -> Option<String> {
    body["refs"].as_array()?.iter().find_map(|reference| {
        reference
            .as_str()?
            .strip_prefix(myriad_nodes::body::MEMORY_REF)
            .map(str::to_string)
    })
}
