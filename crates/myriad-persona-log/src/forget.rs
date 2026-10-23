//! Forgetting absorbs: once an object (or everything about a subject in an
//! audience) is forgotten, no row of it reads back, whether its rows came
//! before the forget, after it, or from a side that was offline when it
//! happened.
//!
//! The ledger holds only what was forgotten, where and when: never a word
//! of what it said. A row on the ledger becomes a stub: its payload goes,
//! its place in the log (id, side, count) stays so vectors still agree.

use std::collections::BTreeMap;

use myriad_persona_sync::Rev;
use serde::{Deserialize, Serialize};

use crate::{Row, kinds::Rule, kinds::SUBJECT};

/// One thing forgotten: an object, or `subject:<id>` for everything about
/// that subject, in one audience.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    pub audience: String,
    pub target: String,
    /// The forget's clock (the earliest, when it was forgotten twice).
    pub at: Rev,
}

/// The ledger the forgets in `rows` make, sorted by audience and target.
pub fn ledger(rows: &[Row]) -> Vec<Entry> {
    let entries: Vec<Entry> = rows
        .iter()
        .filter(|row| row.rule() == Rule::Forget)
        .map(|row| Entry {
            audience: row.audience.clone(),
            target: row.object.clone(),
            at: row.hlc.clone(),
        })
        .collect();
    joined(&[], &entries)
}

/// The ledger with `more` added, sorted, earliest forget kept.
pub fn joined(ledger: &[Entry], more: &[Entry]) -> Vec<Entry> {
    let mut entries: BTreeMap<(&str, &str), &Rev> = BTreeMap::new();
    for entry in ledger.iter().chain(more) {
        let at = entries
            .entry((entry.audience.as_str(), entry.target.as_str()))
            .or_insert(&entry.at);
        if entry.at < **at {
            *at = &entry.at;
        }
    }
    entries
        .into_iter()
        .map(|((audience, target), at)| Entry {
            audience: audience.to_string(),
            target: target.to_string(),
            at: at.clone(),
        })
        .collect()
}

/// Whether `ledger` forgets `row`. A forget itself is never forgotten.
pub fn forgets(ledger: &[Entry], row: &Row) -> bool {
    if row.rule() == Rule::Forget {
        return false;
    }
    let subject = row
        .subject
        .as_deref()
        .map(|subject| format!("{SUBJECT}:{subject}"));
    ledger.iter().any(|entry| {
        entry.audience == row.audience
            && (entry.target == row.object || Some(&entry.target) == subject.as_ref())
    })
}

/// `rows` with every row `ledger` forgets turned into a stub, and the ids
/// of the rows that had a payload until now (whose payload the caller
/// deletes).
pub fn absorb(rows: Vec<Row>, ledger: &[Entry]) -> (Vec<Row>, Vec<String>) {
    let mut stubbed = Vec::new();
    let rows = rows
        .into_iter()
        .map(|row| {
            if row.forgotten || !forgets(ledger, &row) {
                return row;
            }
            stubbed.push(row.id.clone());
            row.stub()
        })
        .collect();
    stubbed.sort();
    (rows, stubbed)
}
