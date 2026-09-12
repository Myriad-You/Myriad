//! Shared "this is still now" cut for live presence.
//!
//! Not a platform config — a config field would also need a DB-read parse branch.

/// A live face older than this is no longer now.
pub const PRESENCE_WINDOW_SECS: i64 = 90;
