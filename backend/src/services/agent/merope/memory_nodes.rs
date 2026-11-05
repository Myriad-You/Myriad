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
//! What writes them from a private talk, while the switch writes (the old
//! topic path goes on beside it in every mode, so turning it off is the
//! way back):
//!
//! - **Placing a stretch** ([`placing`]): once a stretch is gone over,
//!   first she cuts it into spans on her nodes, the unsure ones into the
//!   day's inbox ([`inbox`]).
//! - **What she keeps for good** ([`facts`]): every memory going over the
//!   stretch keeps, an entry on the node it is of, on 「关于你」, or nowhere;
//!   what she knew before, sorted onto them at upkeep ([`sorting`]).
//! - **Her upkeep** ([`upkeep`]): the nodes due, gone over through the
//!   core, with a budget per account.
//! - **Forgetting** ([`forget`]): a forgotten memory's entries, the spans
//!   of its words, and a topic she started left empty.
//! - **Her intents** ([`intending`]): after each message of theirs, what it
//!   leaves her for later: their instruction, or something she wants to
//!   bring up, as an intent on a node ([`intent_store`]); come due, said or,
//!   their instruction checked again, done ([`intents_due`],
//!   [`intents_run`]).
//!
//! What writes them from everything else she keeps, at the place each is
//! written, beside the row:
//!
//! - **Groups**: what going over a stretch in a group keeps, on the group's
//!   nodes ([`facts`]), and what someone there said of themselves also on
//!   「关于你」 in their own private talk ([`group_facts`]).
//! - **Her other sources** ([`hanging`], where each goes: [`homes`]): what
//!   someone is to her and how she comes across as her views, sore spots as
//!   what is not wrapped up, bits, a group's own words and what people told
//!   her about herself as entries, a group's days and her own as spans.
//! - **What is on her mind and what she wants** ([`home_intents`]): a
//!   thread about someone, and a want of hers, as her intent, kept in step
//!   with its row both ways.
//! - **A child end** ([`child`]): what she made of a moment there, on the
//!   site owner's repository or talks-with-other-AIs node.
//!
//! What the site owner sees of her nodes, and the little they may change
//! there (delete, rename, cancel an intent, answer what was heard), is the
//! page ([`page`], [`page_edit`]).
//!
//! What placing, going over and sorting are shown of her nodes is
//! [`known`]. What she is shown of them when she speaks in a private talk,
//! while she reads them: every time, what she keeps of the person and of
//! herself there, what is going on and what she may bring up now
//! ([`context`]); chosen for their words, the nodes they touch and their
//! entries, and all of a kind listed by code ([`recall`]); each entry as
//! far as she may take it ([`shown`]). In a group, that group's audience
//! alone ([`group_context`]).
//!
//! The site keeps no ledger of forgets for these rows: forgetting a memory
//! row takes its placement with it (the foreign key), and its rows on nodes
//! become stubs ([`forget`]). Every fold is given an empty ledger; the
//! forgets the rows themselves hold still count.

// Some of this is read only by the steps that read her nodes into what she
// says; the tests use what they test, the rest waits.
#![allow(dead_code, unused_imports)]

use std::fmt;

use sea_orm::DbErr;

pub(crate) mod audience;
pub(crate) mod child;
pub(crate) mod context;
pub(crate) mod facts;
pub(crate) mod forget;
pub(crate) mod group_context;
pub(crate) mod group_facts;
pub(crate) mod hanging;
mod heads;
pub(crate) mod home_intents;
pub(crate) mod homes;
pub(crate) mod inbox;
pub(crate) mod intending;
pub(crate) mod intent_store;
pub(crate) mod intents_due;
pub(crate) mod intents_run;
pub(crate) mod known;
pub(crate) mod page;
pub(crate) mod page_edit;
mod page_intents;
pub(crate) mod placements;
pub(crate) mod placing;
pub(crate) mod prepare;
pub(crate) mod read;
pub(crate) mod recall;
pub(crate) mod shown;
pub(crate) mod sorting;
pub(crate) mod switch;
pub(crate) mod upkeep;
pub(crate) mod write;

pub(crate) use prepare::{Author, RowIn};
pub(crate) use switch::{Mode, apply_config, mode};
pub(crate) use write::{Beside, Written, write, write_with};

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

#[cfg(test)]
mod talk_tests;

#[cfg(test)]
mod keep_tests;

#[cfg(test)]
mod intent_tests;

#[cfg(test)]
mod source_tests;

#[cfg(test)]
mod home_intent_tests;

#[cfg(test)]
pub(crate) mod read_tests;

#[cfg(test)]
pub(crate) mod group_read_tests;

#[cfg(test)]
mod page_tests;
