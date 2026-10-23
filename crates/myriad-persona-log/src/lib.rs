//! Her log: the rows every host core keeps of her, and the rules that make
//! one her of them on every side.
//!
//! There is one her, and one mind thinking at a time. Every host core (each
//! PC, the server) keeps its own append-only log; nothing in it is ever
//! rewritten. Sides swap version vectors ([`vector`]), send what the other
//! lacks, check what arrives ([`admit`]) and take the union ([`merge`]). What
//! she is now is folded from the log by the same rules everywhere ([`fold`]).
//! Forgetting absorbs: a forgotten object never reads back, whatever order
//! its rows and the forget arrive in ([`forget`]).
//!
//! Her own rows are written only by the holder of the lease and carry its
//! epoch; the owner's rows are written anywhere, offline too, and carry
//! none. Who may move the lease, when a holder stops by itself, and which
//! rows overlap because the owner forced a move are in [`lease`].
//!
//! Its clock is the shared one (`hlc`), fed corrected time and kept from
//! running ahead of the server ([`clock`]).
//!
//! Pure: nothing here reads a clock, a file, the network, a key or a random
//! source. Times come in as Unix microseconds; ids and revisions come from
//! the caller. Content never enters a row: a row only points at its payload.

use std::fmt;

pub mod admit;
pub mod clock;
pub mod fold;
pub mod forget;
pub mod kinds;
pub mod lease;
pub mod merge;
pub mod row;
pub mod vector;

pub use admit::{AdmitInput, Admitted, admit};
pub use fold::{FoldInput, Folded, fold};
pub use forget::{Entry, ledger};
pub use kinds::Rule;
pub use lease::{Grant, Here, How, Moved, Request, Through};
pub use merge::{MergeInput, Merged, merge};
pub use row::{Author, LOG_VERSION, Payload, Replicate, Row};
pub use vector::{Holdings, Span, Vector};

/// Why a row or input was refused. A row holds no content and no secret,
/// and neither does a message about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogError(String);

impl LogError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LogError {}

#[cfg(test)]
mod tests;
