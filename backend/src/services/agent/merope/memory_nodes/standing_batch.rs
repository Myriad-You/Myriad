//! Stretch-batched 「现在」 updates for multi-fact stretches, orchestrated
//! through the shared core (`myriad_nodes::stands::step_batch_*`).

use std::collections::{HashMap, HashSet};

use myriad_nodes::stands::{
    self, BatchStepInput, BatchStepReadInput, KnownIn, NodeIn, StillSo,
};
use sea_orm::DatabaseConnection;

use super::{
    Error,
    facts::Fact,
    placements, read,
    standing::{UNREADABLE, ask, counted, day_of, log_left, write_rows},
};
use crate::services::agent::memory::unified::{self, Audience, MemoryKind};
use crate::services::her_log::pen::Thought;

/// Output a batched stretch step may take, thinking included.
const BATCH_STEP_OUTPUT: u32 = 3072;

pub(super) async fn placement_on(db: &DatabaseConnection, memory: &str) -> Result<Vec<String>, Error> {
    Ok(placements::placement(db, memory)
        .await?
        .and_then(|placement| placement.node_id)
        .into_iter()
        .collect())
}

pub(super) async fn recall_candidates(
    db: &DatabaseConnection,
    user_id: i32,
    present: &Audience,
    facts: &[Fact],
    fact: &Fact,
) -> Result<Vec<(String, KnownIn)>, Error> {
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
    let mut out = Vec::new();
    for record in recalled
        .into_iter()
        .filter(|record| !facts.iter().any(|kept| kept.memory == record.id))
    {
        let on = placement_on(db, &record.id).await?;
        out.push((
            record.id,
            KnownIn {
                text: crate::services::agent::merope::ingest::compact_summary(&record.content),
                on,
            },
        ));
    }
    Ok(out)
}

pub(super) async fn collect_nodes<'a>(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    ids: impl Iterator<Item = &'a String>,
    heads: &HashMap<String, read::Head>,
) -> Result<Vec<NodeIn>, Error> {
    let mut nodes: Vec<NodeIn> = Vec::new();
    for id in ids {
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
    Ok(nodes)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn step_batch(
    db: &DatabaseConnection,
    user_id: i32,
    owner: i32,
    present: &Audience,
    audience: &str,
    facts: &[Fact],
    replaced: &[String],
    heads: &HashMap<String, read::Head>,
    pen: &Thought,
) -> Result<usize, Error> {
    let batch = batch_of(db, user_id, owner, present, audience, facts, replaced, heads).await?;
    let prepared = stands::step_batch_prepare(batch.clone())?;
    let Some(output) = Box::pin(ask(
        user_id,
        "memory_standing_step",
        "standing_step_batch",
        BATCH_STEP_OUTPUT,
        &prepared,
    ))
    .await
    else {
        if !prepared.skip {
            tracing::info!("[memory-nodes] how things stand not asked: no answer");
        }
        return Ok(0);
    };
    let read = stands::step_batch_read(BatchStepReadInput {
        output,
        sync: super::share::sync_for(db, owner).await,
        facts: batch.facts,
        known: batch.known,
        nodes: batch.nodes,
    })?;
    if read.unreadable {
        let total = counted(&UNREADABLE);
        tracing::info!(
            total,
            "[memory-nodes] how things stand not written: the answer read as nothing"
        );
        return Ok(0);
    }
    log_left(None, &read.left);
    let mut by_memory: Vec<(String, Vec<StillSo>)> = Vec::new();
    for one in read.still_so {
        let item = StillSo {
            node: one.node,
            item: one.item,
        };
        match by_memory.iter_mut().find(|(mem, _)| *mem == one.memory) {
            Some((_, list)) => {
                if !list.contains(&item) {
                    list.push(item);
                }
            }
            None => by_memory.push((one.memory, vec![item])),
        }
    }
    let written = write_rows(db, owner, audience, read.rows, false, pen).await;
    for (memory, still) in by_memory {
        super::kept::seen(db, owner, audience, &memory, &still).await;
    }
    Ok(written)
}

#[allow(clippy::too_many_arguments)]
async fn batch_of(
    db: &DatabaseConnection,
    user_id: i32,
    owner: i32,
    present: &Audience,
    audience: &str,
    facts: &[Fact],
    replaced: &[String],
    heads: &HashMap<String, read::Head>,
) -> Result<BatchStepInput, Error> {
    let mut own_all: Vec<Vec<String>> = Vec::with_capacity(facts.len());
    let mut per_fact: Vec<Vec<(String, KnownIn)>> = Vec::with_capacity(facts.len());
    for fact in facts {
        own_all.push(placement_on(db, &fact.memory).await?);
        per_fact.push(recall_candidates(db, user_id, present, facts, fact).await?);
    }
    // Prioritize Nowhere facts first in round-robin so placed facts cannot
    // starve their recall candidates out of the shared KNOWN cap.
    let order: Vec<usize> = (0..facts.len())
        .filter(|i| own_all[*i].is_empty())
        .chain((0..facts.len()).filter(|i| !own_all[*i].is_empty()))
        .collect();
    let mut seen_known: HashSet<String> = HashSet::new();
    let mut known: Vec<KnownIn> = Vec::new();
    let mut round = 0usize;
    while known.len() < stands::KNOWN {
        let mut progressed = false;
        for &idx in &order {
            if known.len() >= stands::KNOWN {
                break;
            }
            if let Some((id, item)) = per_fact[idx].get(round) {
                progressed = true;
                if seen_known.insert(id.clone()) {
                    known.push(item.clone());
                }
            }
        }
        if !progressed {
            break;
        }
        round += 1;
    }
    let ids = own_all
        .iter()
        .flatten()
        .chain(known.iter().flat_map(|k| k.on.iter()));
    let nodes = collect_nodes(db, owner, audience, ids, heads).await?;
    let mems: Vec<String> = facts.iter().map(|fact| fact.memory.clone()).collect();
    let learned = super::known::learned(db, mems).await?;
    let fact_inputs = facts
        .iter()
        .zip(own_all)
        .map(|(fact, on)| stands::FactIn {
            memory: fact.memory.clone(),
            text: fact.text.clone(),
            happened: fact
                .standing
                .happened
                .as_ref()
                .map(|happened| happened.from.clone()),
            learned: learned.get(&fact.memory).and_then(|at| day_of(*at)),
            replaced: replaced.contains(&fact.memory),
            on,
        })
        .collect();
    Ok(BatchStepInput {
        facts: fact_inputs,
        known,
        nodes,
    })
}
