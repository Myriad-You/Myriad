//! What she knows for good of someone in private and has not put on a node
//! yet (kept before nodes were written, or by a going-over whose nodes
//! could not be written), sorted onto her nodes at upkeep: a batch at a
//! time through the core (`myriad_nodes::entries::sort_prepare`,
//! `sort_read`, the same prompt and reading the old topics are sorted by),
//! and put on them as going over a stretch puts what it keeps
//! ([`super::facts::put`]). Billed to whom they are about, with today's
//! budget for sorting: a few people an upkeep, a few batches each.
//!
//! Only while nodes are written, and only while her mind is here. A fact
//! she gave no readable place is asked of again at a later upkeep.

use std::time::Duration;

use myriad_merope::topics::SORT_BATCH;
use myriad_nodes::entries::{self, FactIn, SortInput, SortReadInput, TopicIn};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use super::{Error, audience, facts, known, placements};
use crate::services::agent::memory::unified::{Audience, MemoryKind, MemoryRecord, active_in};
use crate::services::agent::merope::call::{Ask, Voice};

/// People gone over in one upkeep, and how many batches each: today's
/// budget for sorting under topics.
const PEOPLE: i64 = 10;
const ASKS: usize = 3;
const TIMEOUT: Duration = Duration::from_secs(90);
const OPERATION: &str = "memory_node_sort";
/// What she knows for good of someone, which she sorts.
const SORTED: &[&str] = &["chat", "import"];

/// People with private facts on no node yet, at most `at_most`.
async fn with_unplaced<C: ConnectionTrait>(db: &C, at_most: i64) -> Result<Vec<i32>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT m.user_id FROM agent_memories m \
             WHERE m.user_id IS NOT NULL AND m.user_id > 0 AND m.venue = 'private' \
               AND m.source = ANY($1) AND m.kind IN ('fact', 'preference') \
               AND m.invalid_at IS NULL AND m.superseded_by IS NULL \
               AND NOT EXISTS (SELECT 1 FROM agent_node_placements p WHERE p.memory_id = m.id) \
             LIMIT $2",
            [
                SORTED
                    .iter()
                    .map(|source| source.to_string())
                    .collect::<Vec<_>>()
                    .into(),
                at_most.into(),
            ],
        ))
        .await?;
    rows.iter()
        .map(|row| Ok(row.try_get::<i32>("", "user_id")?))
        .collect()
}

/// Sort what is still to be put on a node, for a few people.
pub(crate) async fn sort_missing(db: &DatabaseConnection) {
    if !super::mode().writes() {
        return;
    }
    let owner = crate::services::principal::site_owner_id(db)
        .await
        .ok()
        .flatten();
    for user_id in with_unplaced(db, PEOPLE).await.unwrap_or_default() {
        sort_for(db, user_id, owner, ASKS).await;
    }
}

/// Their private facts on no node yet, oldest first, at most a batch.
async fn unplaced(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
) -> Result<Vec<MemoryRecord>, Error> {
    let ids = placements::unplaced(db, user_id, audience, SORTED, SORT_BATCH as u64 * 4).await?;
    let mut rows = active_in(
        db,
        user_id,
        &Audience::private(user_id),
        &MemoryKind::ABOUT_PERSON,
    )
    .await?;
    rows.retain(|row| ids.contains(&row.id) && SORTED.contains(&row.source.as_str()));
    rows.sort_by_key(|row| (row.created_at, row.id.clone()));
    rows.truncate(SORT_BATCH);
    Ok(rows)
}

/// Up to `asks` batches of what she knows of them in private; how many
/// facts were put where they go. Stops at the first batch that puts none.
pub(crate) async fn sort_for(
    db: &DatabaseConnection,
    user_id: i32,
    site_owner: Option<i32>,
    asks: usize,
) -> usize {
    let Some(pen) = crate::services::her_log::pen::think() else {
        tracing::info!("[memory-nodes] nothing sorted onto nodes: her mind is on another device");
        return 0;
    };
    let audience = audience::private_talk(user_id, site_owner);
    let mut sorted = 0;
    for _ in 0..asks {
        let done = match sort_batch(db, user_id, &audience, &pen).await {
            Ok(done) => done,
            Err(error) => {
                tracing::warn!(%error, "[memory-nodes] could not sort onto nodes");
                0
            }
        };
        sorted += done;
        if done == 0 {
            break;
        }
    }
    if sorted > 0 {
        tracing::info!(
            sorted,
            "[memory-nodes] sorted what she knows onto her nodes"
        );
    }
    sorted
}

async fn sort_batch(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    pen: &crate::services::her_log::pen::Thought,
) -> Result<usize, Error> {
    let rows = unplaced(db, user_id, audience).await?;
    if rows.is_empty() {
        return Ok(0);
    }
    let topics: Vec<known::Known> = known::known(db, user_id, audience)
        .await?
        .into_iter()
        .filter(known::Known::is_topic)
        .collect();
    let prepared = entries::sort_prepare(SortInput {
        topics: topics
            .iter()
            .map(|topic| TopicIn {
                id: topic.id.clone(),
                name: topic.name.clone(),
                aliases: topic.aliases.clone(),
                holds: topic.holds.clone(),
            })
            .collect(),
        facts: rows
            .iter()
            .map(|row| FactIn {
                id: row.id.clone(),
                text: crate::services::agent::merope::store::as_known(row),
            })
            .collect(),
    })?;
    let Ok(raw) = Ask::new(Voice::Thorough, user_id, OPERATION)
        .within(TIMEOUT)
        .json_raw(
            &prepared.system,
            &prepared.input,
            crate::services::agent::merope::chat_remember::sort::SCHEMA_NAME,
            &prepared.schema,
        )
        .await
    else {
        return Ok(0);
    };
    let read = entries::sort_read(SortReadInput {
        output: raw,
        offered: prepared.offered.clone(),
        facts: rows
            .iter()
            .take(prepared.taken)
            .map(|row| row.id.clone())
            .collect(),
        known: topics
            .iter()
            .map(|topic| entries::Known {
                id: topic.id.clone(),
                name: topic.name.clone(),
                aliases: topic.aliases.clone(),
            })
            .collect(),
    })?;
    let facts: Vec<facts::Fact> = read
        .facts
        .into_iter()
        .filter_map(|sorted| {
            let row = rows.iter().find(|row| row.id == sorted.fact)?;
            Some(facts::Fact {
                memory: row.id.clone(),
                text: row.content.clone(),
                said_by: facts::said_by(&row.speaker),
                placement: sorted.placement,
            })
        })
        .collect();
    if facts.is_empty() {
        return Ok(0);
    }
    facts::put(db, user_id, audience, &facts, pen).await
}
