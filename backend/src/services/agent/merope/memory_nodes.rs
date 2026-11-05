//! Her nodes on the site as the shared core keeps them
//! (`migrations/memory_nodes.sql`): the rows of her log about `node:<id>`,
//! one audience at a time, folded by `myriad_nodes::fold` into one head per
//! node. A node is a layer over her memory rows, not a copy of them: the
//! memory rows (`agent_memories` and the other sources) are written as they
//! always were and stay the one place of a memory's strength and recency;
//! an entry on a node carries the memory's text and names its row
//! (`memory:<id>`), so a node reads whole on its own and a fold on another
//! side holds the same bytes.
//!
//! - **Which audience** ([`audience`]): a private talk is
//!   `personal:local-owner` for the site owner and `personal:<user_id>` for
//!   every other account; a group is `group:<id>`; her own is `own:`. Who
//!   said something follows from the audience: its one person is the
//!   `partner`, everyone in a group `other_person`, she is `her`.
//! - **One way in** ([`write`]): a batch of rows for one account and
//!   audience, checked by the core's rules (the body by `Body::read`, kept
//!   as its canonical bytes; the key; where it was said), written in one
//!   transaction under the audience's lock, the same row twice writing
//!   nothing, and every head the batch changes rewritten from a fresh fold.
//!   Her rows need her pen ([`crate::services::her_log::pen::Thought`]);
//!   the person's own rows do not. No model is ever called inside.
//! - **Reads** ([`read`]): an audience's heads, one node, its entries and
//!   the memory rows they name. Where a memory was put, and what is not put
//!   yet ([`placements`]).
//! - **The switch** ([`switch`]): `off` (today's topics, the default),
//!   `shadow` (nodes written too, the old path read), `on` (nodes read).
//!
//! Nothing outside the tests calls this yet: what writes and reads it from
//! talk comes in the next steps, and while the switch is off nothing will.
//!
//! The site keeps no ledger of forgets for these rows yet: forgetting a
//! memory row takes its placement with it (the foreign key), and forgetting
//! on nodes comes with the steps that write them. Every fold is given an
//! empty ledger; the forgets the rows themselves hold still count.

// Nothing calls most of this until the steps that write and read it from
// talk; the tests use what they test, the rest waits.
#![allow(dead_code, unused_imports)]

use std::fmt;

use sea_orm::DbErr;

pub(crate) mod audience;
mod heads;
pub(crate) mod placements;
pub(crate) mod prepare;
pub(crate) mod read;
pub(crate) mod switch;
pub(crate) mod write;

pub(crate) use prepare::{Author, RowIn};
pub(crate) use switch::{Mode, apply_config, mode};
pub(crate) use write::{Written, write};

/// Why rows were not written or a read could not be made. It says what was
/// refused, never with what: a row holds her words and the person's things.
#[derive(Debug)]
pub(crate) enum Error {
    /// A row the core's rules refuse, or the batch around it.
    Refused(String),
    /// Her row without her pen: her mind is not on the site now.
    NoPen,
    Db(DbErr),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(what) => write!(f, "refused: {what}"),
            Self::NoPen => f.write_str("her row needs her pen, and her mind is not here"),
            Self::Db(error) => write!(f, "database: {error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<DbErr> for Error {
    fn from(error: DbErr) -> Self {
        Self::Db(error)
    }
}

impl From<myriad_nodes::NodeError> for Error {
    fn from(error: myriad_nodes::NodeError) -> Self {
        Self::Refused(error.to_string())
    }
}

impl From<crate::services::her_log::Error> for Error {
    fn from(error: crate::services::her_log::Error) -> Self {
        match error {
            crate::services::her_log::Error::Db(error) => Self::Db(error),
            other => Self::Refused(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests;
