//! Recall's arithmetic (`myriad_agent_rules::memory`): BM25 over words and
//! concepts, a memory's strength, spreading activation, and the gap she
//! knows little about; and what a chat line keeps (`myriad_merope::diary`).

use chrono::{DateTime, FixedOffset};
use myriad_agent_rules::{
    Concept,
    memory::{association, lexical, rank, recall, strength},
};
use myriad_merope::diary;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{at, input, opt_at, text};
use crate::Failure;

/// A memory as recall reads it: its text and what it is about.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Memory {
    text: String,
    #[serde(default)]
    concepts: Vec<Concept>,
}

fn documents(memories: &[Memory]) -> Vec<lexical::Document<'_>> {
    memories
        .iter()
        .map(|memory| lexical::Document {
            text: &memory.text,
            concepts: &memory.concepts,
        })
        .collect()
}

pub(super) fn score_all(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        query: String,
        memories: Vec<Memory>,
    }
    let req: In = input(raw)?;
    let scores: Vec<Value> = lexical::score_all(&req.query, &documents(&req.memories))
        .into_iter()
        .map(|score| json!({ "value": score.value, "strong": score.strong }))
        .collect();
    Ok(json!({ "scores": scores }))
}

pub(super) fn overlap(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        a: String,
        b: String,
    }
    let req: In = input(raw)?;
    Ok(json!({ "overlap": lexical::overlap(&req.a, &req.b) }))
}

pub(super) fn gap_in(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        memories: Vec<Memory>,
        query: String,
    }
    let req: In = input(raw)?;
    let gap = recall::gap_in(&documents(&req.memories), &req.query);
    Ok(json!({ "gap": gap.map(|(name, known)| json!({ "name": name, "known": known })) }))
}

/// A UTC moment as the fixed-offset time recall's rules take.
fn fixed(micros: i64) -> Result<DateTime<FixedOffset>, Failure> {
    Ok(at(micros)?.fixed_offset())
}

pub(super) fn spread(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Node {
        #[serde(default)]
        concepts: Vec<Concept>,
        /// When it was learned.
        at: i64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        seeds: Vec<f64>,
        nodes: Vec<Node>,
    }
    let req: In = input(raw)?;
    if req.seeds.len() != req.nodes.len() {
        return Err(Failure::BadInput(format!(
            "{} seeds for {} nodes",
            req.seeds.len(),
            req.nodes.len()
        )));
    }
    let nodes = req
        .nodes
        .iter()
        .map(|node| {
            Ok(association::Node {
                concepts: &node.concepts,
                at: fixed(node.at)?,
            })
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    Ok(json!({ "activation": association::spread(&req.seeds, &nodes) }))
}

pub(super) fn base_level(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        uses: u32,
        lifetime: f64,
        since: f64,
    }
    let req: In = input(raw)?;
    Ok(json!({ "base": strength::base_level(req.uses, req.lifetime, req.since) }))
}

pub(super) fn readiness(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        base: f64,
    }
    let req: In = input(raw)?;
    Ok(json!({ "readiness": strength::readiness(req.base) }))
}

pub(super) fn of_row(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        created_at: i64,
        access_count: i32,
        #[serde(default)]
        last_accessed_at: Option<i64>,
        now: i64,
    }
    let req: In = input(raw)?;
    let last = opt_at(req.last_accessed_at)?.map(|last| last.fixed_offset());
    Ok(json!({
        "readiness": strength::of_row(
            fixed(req.created_at)?,
            req.access_count,
            last,
            fixed(req.now)?,
        ),
    }))
}

/// What she always has in mind about someone (`recall::always_known`).
pub(super) fn always_known(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Row {
        id: String,
        content: String,
        #[serde(default)]
        concepts: Vec<Concept>,
        source: String,
        created_at: i64,
        access_count: i32,
        #[serde(default)]
        last_accessed_at: Option<i64>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        rows: Vec<Row>,
        now: i64,
    }
    let req: In = input(raw)?;
    let mut view = Vec::with_capacity(req.rows.len());
    for row in &req.rows {
        view.push(rank::Row {
            id: &row.id,
            content: &row.content,
            concepts: &row.concepts,
            source: &row.source,
            created_at: fixed(row.created_at)?,
            access_count: row.access_count,
            last_accessed_at: opt_at(row.last_accessed_at)?.map(|at| at.fixed_offset()),
        });
    }
    Ok(json!({ "chosen": recall::always_known(&view, fixed(req.now)?) }))
}

pub(super) fn should_write_chat_diary(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        text: String,
        /// Minutes since her last chat entry, if she has one.
        #[serde(default)]
        last_age_minutes: Option<i64>,
    }
    let req: In = input(raw)?;
    Ok(json!({ "write": diary::should_write_chat_diary(&req.text, req.last_age_minutes) }))
}

pub(super) fn compact_summary(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        text: String,
    }
    let req: In = input(raw)?;
    Ok(text(Some(diary::compact_summary(&req.text))))
}

pub(super) fn redact(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        text: String,
    }
    let req: In = input(raw)?;
    Ok(text(Some(diary::redact_event_text(&req.text))))
}
