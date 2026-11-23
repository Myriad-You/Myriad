//! Whose word her log's newest row of a record is taken for that it is gone
//! (`state.write` deleted), when [`super::state::take_in`] takes it in here.
//!
//! - hers: only from the end that held her at the row's epoch, as a grant the
//!   server kept says, and while it did. Not from a temporary host (what it
//!   wrote while the server was out of its reach is concurrent with her here;
//!   a mood past its days, gone there, goes here by the same rule), nor from
//!   an end that did not hold her, nor one she had moved on from, however she
//!   did, nor at an epoch no grant is of (one written alone, before any);
//! - the owner's: only what the owner changes of her on another end, her
//!   wardrobe;
//! - a rule's: never.
//!
//! One not taken is left in the site's tables as it is, and her log has it
//! live again as the site next writes her records (`state::sync`). One the
//! site does not have stays gone.
//!
//! Who wrote a row, and as whom, is what the end that sent it says: this keeps
//! what an end writes wrongly from taking records away, it is no wall against
//! an end that means to.

use std::collections::BTreeSet;

use myriad_persona_log::{
    Author, Grant, Row,
    lease::{Stand, standing},
};
use myriad_persona_sync::Kind;
use sea_orm::ConnectionTrait;

use super::{Error, deputy, lease};

/// Why a record is not taken as gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refused {
    /// Written by an end that did not hold her, or at an epoch no grant is
    /// of.
    NotHolder,
    AfterHandover,
    /// Written as her temporary host, or beside her elsewhere.
    Concurrent,
    /// Not hers, and not something its author may say is gone.
    NotTheirs,
}

/// What her rows are placed against.
pub(super) struct Holders {
    grants: Vec<Grant>,
    deputies: Vec<deputy::Deputy>,
}

/// What `rows` are placed against, as the server keeps it now.
pub(super) async fn holders<C: ConnectionTrait>(db: &C, rows: &[&Row]) -> Result<Holders, Error> {
    let origins: BTreeSet<&str> = rows.iter().map(|row| row.origin.as_str()).collect();
    let origins: Vec<&str> = origins.into_iter().collect();
    Ok(Holders {
        grants: lease::history(db).await?,
        deputies: deputy::for_holders(db, &origins).await?,
    })
}

impl Holders {
    /// Why `row`, a record of `kind` written as gone, is not taken for it.
    pub(super) fn refuses(&self, row: &Row, kind: Kind) -> Option<Refused> {
        match row.author {
            Author::Owner => {
                (!matches!(kind, Kind::Persona | Kind::Outfit)).then_some(Refused::NotTheirs)
            }
            Author::Rule => Some(Refused::NotTheirs),
            Author::Her => match standing(row, &self.grants, &self.deputies) {
                // Normal is also what a row stands whose epoch no grant is of.
                Stand::Normal => (!self.held(row)).then_some(Refused::NotHolder),
                Stand::Concurrent => Some(Refused::Concurrent),
                Stand::NotHolder => Some(Refused::NotHolder),
                Stand::AfterHandover => Some(Refused::AfterHandover),
            },
        }
    }

    /// Whether a grant kept here says its writer held her at its epoch, and
    /// it wrote this before the next was issued (or by the count it had then):
    /// what comes after a takeover too is past it.
    fn held(&self, row: &Row) -> bool {
        let Some(epoch) = row.epoch else {
            return false;
        };
        let its = |grant: &Grant| grant.epoch == epoch && grant.held_by(&row.origin);
        if !self.grants.iter().any(its) {
            return false;
        }
        let next = self
            .grants
            .iter()
            .filter(|grant| grant.epoch > epoch)
            .min_by_key(|grant| grant.epoch);
        next.is_none_or(|next| {
            let through = next.through.as_ref().and_then(|t| t.get(&row.audience));
            row.hlc <= next.issued
                || matches!((row.dseq, through), (Some(dseq), Some(last)) if dseq <= *last)
        })
    }
}

/// `objects` counted by kind, for a log line: never an id.
pub(super) fn by_kind<'a>(objects: impl Iterator<Item = &'a str>) -> String {
    let mut counts: std::collections::BTreeMap<&str, usize> = Default::default();
    for object in objects {
        let kind = object.split(':').nth(1).unwrap_or("?");
        *counts.entry(kind).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(kind, count)| format!("{kind}={count}"))
        .collect::<Vec<_>>()
        .join(" ")
}
