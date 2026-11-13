//! How their life stands now (「现在」, `node.standing` in the core), written
//! from a private talk on the site. What is asked, how the answer is read
//! and the rows it makes are the core's (`myriad_nodes::stands`, the same on
//! every end); here is only what the site does around it: which facts,
//! what she recalls for each, the nodes as they are folded here, the model
//! call, and writing the rows.
//!
//! - **After a stretch is gone over** ([`after_settled`]): each fact it
//!   kept, one call each, in the order kept.
//! - **What she knew before stands first** ([`build`]): in a talk where none
//!   of hers stands yet, before the first stretch's facts are read, each of
//!   her nodes there with none of hers on it, once. A rule's 「现在」 (where
//!   a session is `now`) is not hers, and the site owner's talk holds those
//!   from the start.
//! - **Kept on the site's side** (`Sync::Local`) while a side of the
//!   owner's would refuse it, written to go once every side reads it
//!   (`share::sync_for`); what was kept before goes then too
//!   (`share::share_kept`, when a side comes).
//!
//! The memory bench's trial of a kept list
//! (`memory_bench::background::state_list`) builds through [`build`] too.

use std::collections::{BTreeMap, HashMap};

use myriad_nodes::stands::{
    self, BuildDueInput, BuildInput, BuildReadInput, FactIn, KnownIn, NodeIn, StandingRowIn,
    StepInput, StepPrepared, StepRead, StepReadInput,
};
use sea_orm::DatabaseConnection;

use super::{Author, Error, RowIn, facts::Fact, placements, read, write};
use crate::services::agent::memory::unified::{self, Audience, MemoryKind};
use crate::services::her_log::pen::Thought;

/// The day of `micros` in her zone, as the core takes a day.
fn day_of(micros: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_micros(micros).map(|at| {
        at.with_timezone(&crate::services::agent::merope::clock::zone())
            .format("%Y-%m-%d")
            .to_string()
    })
}

/// The authors and items of `owner`'s `audience`'s 「现在」 rows, each
/// once: her line on a repository is hers but no one's life, and only the
/// core tells them apart. A forgotten row (a stub, its body gone) stands
/// for nothing.
async fn standing_authors(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
) -> Result<Vec<StandingRowIn>, Error> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT author, \
             CASE WHEN body LIKE '{%' THEN body::jsonb->>'item' END AS item FROM agent_node_rows \
             WHERE user_id = $1 AND audience = $2 AND kind = $3 AND body <> ''",
            [owner.into(), audience.into(), stands::KIND.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok(StandingRowIn {
                author: row.try_get("", "author")?,
                item: row.try_get("", "item")?,
            })
        })
        .collect()
}

/// Asks the fast model what `prepared` asks: its answer as it came, or none
/// (nothing to ask, or no answer).
async fn ask(
    user_id: i32,
    operation: &'static str,
    schema_name: &str,
    prepared: &StepPrepared,
) -> Option<String> {
    if prepared.skip {
        return None;
    }
    super::super::call::Ask::new(super::super::call::Voice::Judge, user_id, operation)
        .json_raw(
            &prepared.system,
            &prepared.input,
            schema_name,
            &prepared.schema,
        )
        .await
        .ok()
}

/// Writes `read`'s rows, a node at a time: a node that cannot take its rows
/// leaves the others. How many were written.
async fn write_rows(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    read: StepRead,
    pen: &Thought,
) -> usize {
    let mut by_node: Vec<(String, Vec<RowIn>)> = Vec::new();
    for row in read.rows {
        let row = RowIn {
            id: row.id,
            node: row.node,
            kind: row.kind,
            body: row.body,
            author: Author::Her,
        };
        match by_node.iter_mut().find(|(at, _)| *at == row.node) {
            Some((_, rows)) => rows.push(row),
            None => by_node.push((row.node.clone(), vec![row])),
        }
    }
    let mut written = 0;
    for (node, rows) in by_node {
        match write::write(db, owner, audience, &rows, Some(pen)).await {
            Ok(_) => written += rows.len(),
            Err(error) => {
                tracing::warn!(%error, node, "[memory-nodes] how things stand not written")
            }
        }
    }
    written
}

/// How things stand on each of her nodes of `audience` (`owner`'s) that
/// holds none of hers yet, from what she knows there: a topic's, and
/// theirs (「关于你」). One call to the fast model per node with something
/// on it; `leave_out` are memories not to go by (what a stretch just kept,
/// which the step after then reads as new). How many rows were written.
pub(crate) async fn build(
    db: &DatabaseConnection,
    user_id: i32,
    owner: i32,
    audience: &str,
    leave_out: &[String],
    pen: &Thought,
) -> Result<usize, Error> {
    let mut written = 0;
    for head in read::heads(db, owner, audience, None).await? {
        if !matches!(head.kind.as_deref(), Some("topic") | Some("person")) {
            continue;
        }
        let Some(folded) = read::node(db, owner, audience, &head.node_id).await? else {
            continue;
        };
        let ids: Vec<String> = read::entries_of(&folded)
            .into_iter()
            .flat_map(|entry| entry.memories)
            .collect();
        let held: Vec<String> = super::known::held(db, ids.clone())
            .await?
            .into_iter()
            .filter(|id| !leave_out.contains(id))
            .collect();
        let learned: BTreeMap<String, String> = super::known::learned(db, ids)
            .await?
            .into_iter()
            .filter_map(|(id, at)| Some((id, day_of(at)?)))
            .collect();
        let input = BuildInput {
            node: NodeIn {
                id: head.node_id.clone(),
                name: head.name.clone(),
                kind: head.kind.clone(),
                folded,
            },
            held,
            learned,
        };
        let prepared = stands::build_prepare(input.clone())?;
        if prepared.skip {
            continue;
        }
        let Some(output) = ask(
            user_id,
            "memory_standing_build",
            "standing_build",
            &prepared,
        )
        .await
        else {
            tracing::info!("[memory-nodes] how things stand on a node not built: no answer");
            continue;
        };
        let read = stands::build_read(BuildReadInput {
            output,
            sync: super::share::sync_for(db, owner).await,
            node: input.node,
            held: input.held,
            learned: input.learned,
        })?;
        if read.unreadable {
            tracing::info!("[memory-nodes] how things stand on a node not built: no answer");
            continue;
        }
        written += write_rows(db, owner, audience, read, pen).await;
    }
    Ok(written)
}

/// What each fact a stretch of `user_id`'s private talk kept does to how
/// their life stands, written on the nodes of `audience` (`owner`'s): one
/// call to the fast model per fact on a node, in the order they were kept.
/// `replaced` are the facts that took the place of something she knew.
/// How many rows were written.
pub(crate) async fn after_settled(
    db: &DatabaseConnection,
    user_id: i32,
    owner: i32,
    present: &Audience,
    audience: &str,
    facts: &[Fact],
    replaced: &[String],
    pen: &Thought,
) -> Result<usize, Error> {
    let mut written = 0;
    // None of hers stands here yet (what she kept before 「现在」 was
    // written): first how things stood before this stretch, so what it
    // changes is changed, not only started.
    let due = BuildDueInput {
        facts: facts.len(),
        standing: standing_authors(db, owner, audience).await?,
    };
    if stands::build_due(&due) {
        let kept: Vec<String> = facts.iter().map(|fact| fact.memory.clone()).collect();
        written += build(db, user_id, owner, audience, &kept, pen).await?;
    }
    let heads: HashMap<String, read::Head> = read::heads(db, owner, audience, None)
        .await?
        .into_iter()
        .map(|head| (head.node_id.clone(), head))
        .collect();
    for fact in facts {
        let step = step_of(
            db, user_id, owner, present, audience, facts, fact, replaced, &heads,
        )
        .await?;
        let prepared = stands::step_prepare(step.clone())?;
        let Some(output) = ask(user_id, "memory_standing_step", "standing_step", &prepared).await
        else {
            if !prepared.skip {
                tracing::info!("[memory-nodes] how things stand not asked: no answer");
            }
            continue;
        };
        let read = stands::step_read(StepReadInput {
            output,
            sync: super::share::sync_for(db, owner).await,
            fact: step.fact,
            known: step.known,
            nodes: step.nodes,
        })?;
        if read.unreadable {
            tracing::info!("[memory-nodes] how things stand not asked: no answer");
            continue;
        }
        // Each item left as another person's: which, and whose on each
        // side; ids and keys, never the words.
        for left in &read.left {
            tracing::info!(
                memory = %fact.memory,
                item = %left.item,
                item_whose = %left.item_whose,
                fact_whose = %left.fact_whose,
                "[memory-nodes] how another person's life stands left as it is"
            );
        }
        written += write_rows(db, owner, audience, read, pen).await;
    }
    Ok(written)
}

/// What the core is given of one fact: the fact, what it brings to mind
/// (read quietly, not her remembering; not what this stretch kept), and
/// the nodes either is kept on, folded.
#[allow(clippy::too_many_arguments)]
async fn step_of(
    db: &DatabaseConnection,
    user_id: i32,
    owner: i32,
    present: &Audience,
    audience: &str,
    facts: &[Fact],
    fact: &Fact,
    replaced: &[String],
    heads: &HashMap<String, read::Head>,
) -> Result<StepInput, Error> {
    let on = |memory: String| async move {
        Ok::<_, Error>(
            placements::placement(db, &memory)
                .await?
                .and_then(|placement| placement.node_id)
                .into_iter()
                .collect::<Vec<String>>(),
        )
    };
    let own = on(fact.memory.clone()).await?;
    let (recalled, _) = unified::quietly(unified::recall_primed(
        db,
        user_id,
        present,
        Some(&fact.text),
        &MemoryKind::ABOUT_PERSON,
        stands::KNOWN * 2,
        &Default::default(),
        1.0,
    ))
    .await?;
    let mut known: Vec<KnownIn> = Vec::new();
    for record in recalled
        .into_iter()
        .filter(|record| !facts.iter().any(|kept| kept.memory == record.id))
        .take(stands::KNOWN)
    {
        known.push(KnownIn {
            text: crate::services::agent::merope::ingest::compact_summary(&record.content),
            on: on(record.id.clone()).await?,
        });
    }
    let mut nodes: Vec<NodeIn> = Vec::new();
    for id in own
        .iter()
        .chain(known.iter().flat_map(|known| known.on.iter()))
    {
        if nodes.iter().any(|node| &node.id == id) {
            continue;
        }
        if let Some(folded) = read::node(db, owner, audience, id).await? {
            let head = heads.get(id);
            nodes.push(NodeIn {
                id: id.clone(),
                name: head.and_then(|head| head.name.clone()),
                kind: head.and_then(|head| head.kind.clone()),
                folded,
            });
        }
    }
    let learned = super::known::learned(db, vec![fact.memory.clone()])
        .await?
        .get(&fact.memory)
        .and_then(|at| day_of(*at));
    Ok(StepInput {
        fact: FactIn {
            memory: fact.memory.clone(),
            text: fact.text.clone(),
            happened: fact
                .standing
                .happened
                .as_ref()
                .map(|happened| happened.from.clone()),
            learned,
            replaced: replaced.contains(&fact.memory),
            on: own,
        },
        known,
        nodes,
    })
}
