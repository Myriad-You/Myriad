//! Backward-compatible deserialization for legacy self-update outcome files.
//!
//! Guard/updater self-update execution was removed when Guard became an
//! independently operated TCB. This module intentionally contains no command,
//! Docker, environment mutation, or status-writing capability.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SelfUpdateOutcome {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelfUpdateLastStatus {
    pub status: SelfUpdateOutcome,
    pub target_tag: String,
    pub previous_tag: String,
    /// RFC3339 UTC timestamp written by a legacy release.
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
