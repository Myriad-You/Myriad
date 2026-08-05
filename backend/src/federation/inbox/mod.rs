//! Federation inbox: signed Activity receive paths and activity handlers.
//!
//! Implementation lives in [`receive`] (formerly multi-file include! split).

mod receipt;
mod receive;

pub use receive::*;
