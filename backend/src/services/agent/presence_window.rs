//! Shared "this is still now" cut for chat occupancy and live presence.
//!
//! Not a platform config — a config field would also need a DB-read parse branch.

/// A live face older than this is no longer now; chatting without an open run
/// expires at the same cut.
pub const PRESENCE_WINDOW_SECS: i64 = 90;
