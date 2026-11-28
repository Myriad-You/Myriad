//! Stretch-batched 「现在」 updates for multi-fact stretches, orchestrated
//! through the shared core (`myriad_nodes::stands::step_batch_*`).

use std::collections::{HashMap, HashSet};

use myriad_nodes::stands::{
    self, BatchStepInput, BatchStepReadInput, KnownIn, NodeIn, PersonIn, StillSo,
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

/// The people of theirs the audience keeps a node for (`heads`), each with
/// the name the node goes by and its other names, by node id: a fact naming
/// one of them another way (老妈 for 妈妈) still changes what stands of
/// them. A node gone (absorbed, forgotten, taken back) names no one.
pub(super) fn people_of(heads: &HashMap<String, read::Head>) -> Vec<PersonIn> {
    let mut people: Vec<(&str, PersonIn)> = heads
        .values()
        .filter(|head| head.kind.as_deref() == Some("person"))
        .filter(|head| !matches!(head.status.as_str(), "absorbed" | "forgotten" | "retracted"))
        .filter_map(|head| {
            let person = PersonIn {
                name: head.name.clone()?,
                aliases: head.aliases.clone(),
            };
            Some((head.node_id.as_str(), person))
        })
        .collect();
    people.sort_by(|one, other| one.0.cmp(other.0));
    people.into_iter().map(|(_, person)| person).collect()
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
        people: batch.people,
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
        people: people_of(heads),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(id: &str, kind: &str, name: &str, aliases: &[&str], status: &str) -> read::Head {
        read::Head {
            node_id: id.into(),
            node_key: None,
            kind: Some(kind.into()),
            name: Some(name.into()),
            aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
            status: status.into(),
        }
    }

    /// The people the core is told of are the audience's live person
    /// nodes, each with its names; with them a fact calling 妈妈 老妈
    /// ends what stands of her instead of leaving it as another person's.
    #[test]
    fn a_fact_calling_a_person_by_another_of_her_names_ends_what_stands_of_her() {
        let heads: HashMap<String, read::Head> = [
            head("n-mom", "person", "妈妈", &["Mom", "老妈"], "active"),
            head("n-run", "topic", "跑步", &["慢跑"], "active"),
            head("n-old", "person", "婆婆", &["老妈"], "absorbed"),
        ]
        .into_iter()
        .map(|head| (head.node_id.clone(), head))
        .collect();
        let people = people_of(&heads);
        assert_eq!(
            people,
            [PersonIn {
                name: "妈妈".into(),
                aliases: vec!["Mom".into(), "老妈".into()],
            }]
        );
        // 妈妈's item: `p` and the hex of her name.
        let item = "pe5a688e5a688-abcdef0123";
        let folded = serde_json::json!({"entries": [], "standing": {
            "pe5a688e5a688-abcdef0123": {
                "base": ["u-pe5a688e5a688-abcdef0123"],
                "value": {"text": "妈妈在社区合唱团唱歌"}
            }
        }});
        let read = |people: Vec<PersonIn>| {
            stands::step_read(stands::StepReadInput {
                output: r#"{"whose": "老妈", "set": [], "end": [1], "add": []}"#.into(),
                sync: myriad_nodes::tags::Sync::Local("site".into()),
                fact: stands::FactIn {
                    memory: "m2".into(),
                    text: "老妈不去合唱团了".into(),
                    happened: None,
                    learned: Some("2026-11-20".into()),
                    replaced: false,
                    on: vec!["n-mom".into()],
                },
                known: Vec::new(),
                nodes: vec![NodeIn {
                    id: "n-mom".into(),
                    name: Some("妈妈".into()),
                    kind: Some("person".into()),
                    folded: folded.clone(),
                }],
                people,
            })
            .unwrap()
        };
        let got = read(people);
        assert!(got.left.is_empty(), "{:?}", got.left);
        assert_eq!(got.rows.len(), 1);
        assert_eq!(got.rows[0].body["item"], serde_json::json!(item));
        assert_eq!(got.rows[0].body["ended"], serde_json::json!(true));
        // Told of no one, 老妈 is another person's.
        assert_eq!(read(Vec::new()).left.len(), 1);
    }
}
