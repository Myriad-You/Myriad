//! What a side holds, as a version vector, and what the other lacks.
//!
//! Each side numbers its shared rows 1, 2, 3… without gaps (`dseq`), so
//! "every row of `origin` up to n" says all a side holds of it. Two sides
//! swap vectors and send each other what is past the other's number; no
//! content is compared.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    Row,
    kinds::Rule,
    row::{Replicate, sort},
};

/// For each side, the last `dseq` held with every one before it.
pub type Vector = BTreeMap<String, u64>;

/// What a log holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Holdings {
    pub vector: Vector,
    /// Rows held past a gap, by side: they arrived before a row ahead of
    /// them did. The vector does not count them until the gap fills.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub beyond: BTreeMap<String, Vec<u64>>,
}

/// What `rows` hold. Local rows hold no place in it.
pub fn holdings(rows: &[Row]) -> Holdings {
    let mut held: BTreeMap<&str, BTreeSet<u64>> = BTreeMap::new();
    for row in rows {
        if let (Replicate::Shared, Some(dseq)) = (row.replicate, row.dseq) {
            held.entry(&row.origin).or_default().insert(dseq);
        }
    }
    let mut vector = Vector::new();
    let mut beyond = BTreeMap::new();
    for (origin, seqs) in held {
        let mut through = 0;
        while seqs.contains(&(through + 1)) {
            through += 1;
        }
        if through > 0 {
            vector.insert(origin.to_string(), through);
        }
        let past: Vec<u64> = seqs.range(through + 1..).copied().collect();
        if !past.is_empty() {
            beyond.insert(origin.to_string(), past);
        }
    }
    Holdings { vector, beyond }
}

/// A run of one side's rows: `from` through `through`, both included.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Span {
    pub origin: String,
    pub from: u64,
    pub through: u64,
}

/// What `theirs` holds that `mine` does not, by side.
pub fn missing(mine: &Vector, theirs: &Vector) -> Vec<Span> {
    theirs
        .iter()
        .filter_map(|(origin, &through)| {
            let have = mine.get(origin).copied().unwrap_or(0);
            (through > have).then(|| Span {
                origin: origin.clone(),
                from: have + 1,
                through,
            })
        })
        .collect()
}

/// The rows of `rows` a side holding `theirs` lacks, in the order to send
/// them: every forget first, so the other side knows what is forgotten
/// before anything it might have to forget arrives; then by side and count.
/// Local rows never go.
pub fn answer(rows: &[Row], theirs: &Vector) -> Vec<Row> {
    let lacking = |row: &&Row| {
        row.replicate == Replicate::Shared
            && row.dseq.unwrap_or(0) > theirs.get(&row.origin).copied().unwrap_or(0)
    };
    let mut forgets: Vec<Row> = rows
        .iter()
        .filter(lacking)
        .filter(|row| row.rule() == Rule::Forget)
        .cloned()
        .collect();
    sort(&mut forgets);
    let mut rest: Vec<Row> = rows
        .iter()
        .filter(lacking)
        .filter(|row| row.rule() != Rule::Forget)
        .cloned()
        .collect();
    rest.sort_by(|a, b| (&a.origin, a.dseq, &a.id).cmp(&(&b.origin, b.dseq, &b.id)));
    let mut seen = BTreeSet::new();
    forgets
        .into_iter()
        .chain(rest)
        .filter(|row| seen.insert(row.id.clone()))
        .collect()
}
