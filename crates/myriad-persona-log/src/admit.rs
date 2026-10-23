//! Checking a batch of rows from another side before it goes into the log.
//!
//! If any row's clock is more than five minutes ahead of this side's, the
//! whole batch is refused and nothing is observed (`hlc::check_ahead`): one
//! row from the future would win every later change. Otherwise each row is
//! taken or refused on its own, and a refused row is only counted. A row
//! already forgotten here is taken as a stub, so the vector still fills.
//!
//! One side's her rows in one audience never go down in epoch along its own
//! order: a side writes [`crate::lease::ALONE`] only before it knows any
//! lease, so those rows come before its first epoch in its count there, and
//! arrive first. Each audience is counted and sent on its own, so the check
//! is per audience: alone rows of one audience may well arrive after
//! another audience's later rows.

use std::collections::{BTreeMap, BTreeSet};

use myriad_persona_sync::hlc;
use serde::{Deserialize, Serialize};

use crate::{
    Row,
    forget::{self, Entry},
    row::{Audience, Replicate, sort},
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdmitInput {
    /// The batch, as it arrived.
    pub rows: Vec<Row>,
    /// This side's clock.
    pub now: i64,
    /// The audience of every object this side already holds rows of (only
    /// the objects the batch touches are needed). An object never changes
    /// audience.
    #[serde(default)]
    pub known: BTreeMap<String, String>,
    /// Who is in a group, for the groups this side knows; a subject outside
    /// them is refused. Groups not listed are not checked.
    #[serde(default)]
    pub members: BTreeMap<String, BTreeSet<String>>,
    /// This side's ledger.
    #[serde(default)]
    pub ledger: Vec<Entry>,
    /// For each audience, then each side, the highest epoch of its her
    /// rows this side holds there.
    #[serde(default)]
    pub epochs: Epochs,
}

/// Audience → side → the highest epoch of that side's her rows held there.
pub type Epochs = BTreeMap<String, BTreeMap<String, u64>>;

/// Why one row was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The envelope is not whole (see `detail`).
    Envelope,
    /// A local row: it never leaves the side that wrote it.
    Local,
    /// Its object is in another audience.
    AudienceChanged,
    /// Its subject is not in the group.
    NotAMember,
    /// Her row with a lower epoch than one its side wrote before it in the
    /// same audience (an alone row after the side knew a lease, most often).
    EpochBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    pub id: String,
    pub reason: Reason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// The whole batch refused: a clock too far ahead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClockAhead {
    pub ahead_ms: u64,
    pub max_ahead_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Admitted {
    /// The rows to merge, in log order; forgotten ones already stubs.
    pub accepted: Vec<Row>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refused: Vec<Refusal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_ahead: Option<ClockAhead>,
    /// This side's ledger with the batch's forgets added.
    pub ledger: Vec<Entry>,
    /// `epochs` with the batch's her rows added.
    pub epochs: Epochs,
}

pub fn admit(input: AdmitInput) -> Admitted {
    let AdmitInput {
        mut rows,
        now,
        mut known,
        members,
        ledger,
        mut epochs,
    } = input;
    let ahead = rows
        .iter()
        .map(|row| hlc::ahead_ms(&row.hlc, now))
        .max()
        .unwrap_or(0);
    if ahead > hlc::MAX_AHEAD_MS {
        return Admitted {
            accepted: Vec::new(),
            refused: Vec::new(),
            clock_ahead: Some(ClockAhead {
                ahead_ms: ahead,
                max_ahead_ms: hlc::MAX_AHEAD_MS,
            }),
            ledger,
            epochs,
        };
    }
    sort(&mut rows);
    let refuse = |row: &Row, reason, detail: Option<String>| Refusal {
        id: row.id.clone(),
        reason,
        detail,
    };
    let mut accepted = Vec::new();
    let mut refused = Vec::new();
    for row in rows {
        if let Err(error) = row.check() {
            refused.push(refuse(&row, Reason::Envelope, Some(error.to_string())));
            continue;
        }
        if row.replicate == Replicate::Local {
            refused.push(refuse(&row, Reason::Local, None));
            continue;
        }
        if let Some(epoch) = row.epoch
            && epochs
                .get(&row.audience)
                .and_then(|sides| sides.get(&row.origin))
                .is_some_and(|&held| epoch < held)
        {
            refused.push(refuse(&row, Reason::EpochBack, None));
            continue;
        }
        if known
            .get(&row.object)
            .is_some_and(|audience| *audience != row.audience)
        {
            refused.push(refuse(&row, Reason::AudienceChanged, None));
            continue;
        }
        let outside = match (Audience::parse(&row.audience), row.subject.as_deref()) {
            (Ok(Audience::Group(_)), Some(subject)) => members
                .get(&row.audience)
                .is_some_and(|group| !group.contains(subject)),
            _ => false,
        };
        if outside {
            refused.push(refuse(&row, Reason::NotAMember, None));
            continue;
        }
        known
            .entry(row.object.clone())
            .or_insert_with(|| row.audience.clone());
        if let Some(epoch) = row.epoch {
            let held = epochs
                .entry(row.audience.clone())
                .or_default()
                .entry(row.origin.clone())
                .or_insert(epoch);
            *held = (*held).max(epoch);
        }
        accepted.push(row);
    }
    let ledger = forget::joined(&ledger, &forget::ledger(&accepted));
    let (accepted, _) = forget::absorb(accepted, &ledger);
    Admitted {
        accepted,
        refused,
        clock_ahead: None,
        ledger,
        epochs,
    }
}
