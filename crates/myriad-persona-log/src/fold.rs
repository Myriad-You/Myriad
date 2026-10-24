//! Folding the log into what she is now, by the same rules on every side.
//!
//! The log is deduplicated by id, put in log order, and folded object by
//! object ([`crate::kinds`] says how each kind folds):
//!
//! - what happened (seen, judged, said, a segment and each of its lines, a
//!   case, a memory written) all stays, in log order;
//! - what changes (her view of a node, what a thing hangs on, a persona
//!   field, an outfit) takes the last row as current, the rest as history;
//! - a memory retired stays retired unless restored later;
//! - a forgotten object folds to nothing but the mark that it was, whatever
//!   order its rows and the forget came in.
//!
//! Her rows interleave only when the owner forced the lease while the old
//! holder kept running, or a side was used alone while she already lived
//! elsewhere; those rows stay and the objects they touch are marked
//! `concurrent` ([`crate::lease::standing`]). Owner rows written on several
//! sides at once just sort by clock.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    LogError, Row, forget,
    kinds::Rule,
    lease::{self, Grant, Stand, Standing},
    row::sort,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FoldInput {
    /// The log, or the part of it to fold (whole objects, with the forgets
    /// that reach them).
    pub rows: Vec<Row>,
    /// The lease history this side knows, for marking her rows.
    #[serde(default)]
    pub leases: Vec<Grant>,
}

/// One object as its rows stand now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectFold {
    pub audience: String,
    pub object: String,
    /// Forgotten: nothing else about it reads back.
    #[serde(skip_serializing_if = "is_false")]
    pub forgotten: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Every row, in log order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<String>,
    /// For each kind that changes: the row that is current.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub current: BTreeMap<String, String>,
    /// For each kind that changes: the rows before the current one.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub history: BTreeMap<String, Vec<String>>,
    /// Whether it is retired, when it was ever retired or restored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retired: Option<bool>,
    /// Some of its rows were written while she was in two places.
    #[serde(skip_serializing_if = "is_false")]
    pub concurrent: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl ObjectFold {
    fn empty(audience: &str, object: &str) -> Self {
        Self {
            audience: audience.to_string(),
            object: object.to_string(),
            forgotten: false,
            subject: None,
            rows: Vec::new(),
            current: BTreeMap::new(),
            history: BTreeMap::new(),
            retired: None,
            concurrent: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folded {
    /// By audience, then object.
    pub objects: Vec<ObjectFold>,
    /// Her rows that do not stand normal against the lease, in log order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub standing: Vec<Standing>,
}

/// One row per id: a stub beats the row it was; two different rows with
/// one id are refused.
pub fn dedupe(rows: Vec<Row>) -> Result<Vec<Row>, LogError> {
    let mut by_id: BTreeMap<String, Row> = BTreeMap::new();
    for row in rows {
        match by_id.get(&row.id) {
            None => {
                by_id.insert(row.id.clone(), row);
            }
            Some(held) if *held == row || held.is_stub_of(&row) => {}
            Some(held) if row.is_stub_of(held) => {
                by_id.insert(row.id.clone(), row);
            }
            Some(_) => {
                return Err(LogError::new(format!(
                    "two different rows have id {:?}",
                    row.id
                )));
            }
        }
    }
    let mut rows: Vec<Row> = by_id.into_values().collect();
    sort(&mut rows);
    Ok(rows)
}

pub fn fold(input: FoldInput) -> Result<Folded, LogError> {
    for row in &input.rows {
        row.check()
            .map_err(|error| LogError::new(format!("row {:?}: {error}", row.id)))?;
    }
    for grant in &input.leases {
        grant.check()?;
    }
    let rows = dedupe(input.rows)?;
    let ledger = forget::ledger(&rows);
    let (rows, _) = forget::absorb(rows, &ledger);
    let standing = lease::standings(&rows, &input.leases);
    let concurrent: Vec<&str> = standing
        .iter()
        .filter(|standing| standing.standing == Stand::Concurrent)
        .map(|standing| standing.id.as_str())
        .collect();

    let mut objects: BTreeMap<(String, String), ObjectFold> = BTreeMap::new();
    for row in &rows {
        let object = objects
            .entry((row.audience.clone(), row.object.clone()))
            .or_insert_with(|| ObjectFold::empty(&row.audience, &row.object));
        if row.rule() == Rule::Forget || row.forgotten {
            object.forgotten = true;
            continue;
        }
        if object.subject.is_none() {
            object.subject = row.subject.clone();
        }
        object.rows.push(row.id.clone());
        object.concurrent |= concurrent.contains(&row.id.as_str());
        match row.rule() {
            Rule::Latest => {
                if let Some(previous) = object.current.insert(row.kind.clone(), row.id.clone()) {
                    object
                        .history
                        .entry(row.kind.clone())
                        .or_default()
                        .push(previous);
                }
            }
            Rule::Retire => object.retired = Some(true),
            Rule::Restore => object.retired = Some(false),
            Rule::Event | Rule::Forget => {}
        }
    }
    let objects = objects
        .into_values()
        .map(|object| {
            if object.forgotten {
                ObjectFold {
                    forgotten: true,
                    ..ObjectFold::empty(&object.audience, &object.object)
                }
            } else {
                object
            }
        })
        .collect();
    Ok(Folded { objects, standing })
}
