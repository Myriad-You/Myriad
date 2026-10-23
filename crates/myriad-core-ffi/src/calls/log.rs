//! Her log across host cores (`myriad_persona_log`): a row's one byte
//! form, what a side holds and lacks, checking and merging what arrives,
//! the fold, the lease and the clock. The rules are the crate's; these calls only
//! carry them across.

use myriad_persona_log::{
    Grant, Row, Vector, admit as admit_rows, clock, fold as fold_rows, lease, merge as merge_rows,
    vector,
};
use myriad_persona_sync::Rev;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::input;
use crate::Failure;

fn refused(error: impl std::fmt::Display) -> Failure {
    Failure::BadInput(error.to_string())
}

fn answer(value: &impl Serialize) -> Result<Value, Failure> {
    serde_json::to_value(value).map_err(refused)
}

/// One row in, checked: the row, its canonical bytes as a string and its
/// digest.
pub(super) fn canonical(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        row: Row,
    }
    let In { row } = input(raw)?;
    row.check().map_err(refused)?;
    let canonical = String::from_utf8(row.canonical_bytes())
        .map_err(|_| refused("canonical bytes are UTF-8"))?;
    Ok(json!({ "row": row, "canonical": canonical, "digest": row.digest() }))
}

/// `{rows}` → `{vector, beyond?}`.
pub(super) fn holdings(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        rows: Vec<Row>,
    }
    let In { rows } = input(raw)?;
    answer(&vector::holdings(&rows))
}

/// `{mine, theirs}` → `{missing: [{origin, from, through}]}`.
pub(super) fn missing(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        mine: Vector,
        theirs: Vector,
    }
    let In { mine, theirs } = input(raw)?;
    Ok(json!({ "missing": vector::missing(&mine, &theirs) }))
}

/// `{rows, theirs}` → `{rows}`: what to send a side holding `theirs`.
pub(super) fn send(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        rows: Vec<Row>,
        theirs: Vector,
    }
    let In { rows, theirs } = input(raw)?;
    Ok(json!({ "rows": vector::answer(&rows, &theirs) }))
}

pub(super) fn admit(raw: &[u8]) -> Result<Value, Failure> {
    answer(&admit_rows(input(raw)?))
}

pub(super) fn merge(raw: &[u8]) -> Result<Value, Failure> {
    answer(&merge_rows(input(raw)?))
}

pub(super) fn fold(raw: &[u8]) -> Result<Value, Failure> {
    answer(&fold_rows(input(raw)?).map_err(refused)?)
}

/// `{sent, serverTime, received}` (local, server, local; microseconds) →
/// `{offset, uncertainty}`.
pub(super) fn clock_offset(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        sent: i64,
        server_time: i64,
        received: i64,
    }
    let In {
        sent,
        server_time,
        received,
    } = input(raw)?;
    answer(&clock::offset(sent, server_time, received).map_err(refused)?)
}

/// `{last?, now, offset, node}` → `{rev, poisoned}`: the stamp for a write
/// at corrected time, and whether it is still too far ahead to send.
pub(super) fn clock_next(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        #[serde(default)]
        last: Option<Rev>,
        now: i64,
        offset: i64,
        node: String,
    }
    let In {
        last,
        now,
        offset,
        node,
    } = input(raw)?;
    let rev = clock::next(last.as_ref(), now, offset, &node).map_err(refused)?;
    let poisoned = clock::poisoned(&rev, now, offset).map_err(refused)?;
    Ok(json!({ "rev": rev, "poisoned": poisoned }))
}

/// `{last?, seen, now, offset, node}` → `{last}`.
pub(super) fn clock_observe(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        #[serde(default)]
        last: Option<Rev>,
        seen: Rev,
        now: i64,
        offset: i64,
        node: String,
    }
    let In {
        last,
        seen,
        now,
        offset,
        node,
    } = input(raw)?;
    let last = clock::observe(last.as_ref(), &seen, now, offset, &node).map_err(refused)?;
    Ok(json!({ "last": last }))
}

pub(super) fn clock_repair(raw: &[u8]) -> Result<Value, Failure> {
    answer(&clock::repair(input(raw)?).map_err(refused)?)
}

pub(super) fn lease_move(raw: &[u8]) -> Result<Value, Failure> {
    answer(&lease::decide(input(raw)?).map_err(refused)?)
}

pub(super) fn lease_here(raw: &[u8]) -> Result<Value, Failure> {
    answer(&lease::here(input(raw)?).map_err(refused)?)
}

/// `{a, b}` → `{newer: "a" | "b"}`.
pub(super) fn lease_newer(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        a: Grant,
        b: Grant,
    }
    let In { a, b } = input(raw)?;
    a.check().map_err(refused)?;
    b.check().map_err(refused)?;
    let newer = if lease::newer(&a, &b) == &a { "a" } else { "b" };
    Ok(json!({ "newer": newer }))
}
