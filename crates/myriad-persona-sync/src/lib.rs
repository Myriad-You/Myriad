//! Her data across the site and the other platforms: the format it crosses
//! in, its one byte form, the clock its revisions come from, and the
//! comparison of two of her before they first merge.
//!
//! The Rust types here are the only definition of the format. A platform
//! turns its own records into rough entities and hands them to
//! [`canonicalize`] (through the shared core); what it stores, sends and
//! compares are the bytes this crate writes, so both sides agree to the bit.
//!
//! Pure: nothing here reads a clock, a file, the network or a key. Times
//! come in as Unix microseconds.

use std::fmt;

pub mod affect;
pub mod canonical;
pub mod compare;
pub mod entity;
pub mod hlc;
pub mod persona;
mod rig;

pub use affect::AffectImpulse;
pub use canonical::{digest, write_canonical_json};
pub use compare::{Comparison, Holdings, compare, is_empty};
pub use entity::{
    AssetRef, AssetType, EntityVersion, Envelope, FORMAT, FORMAT_VERSION, Kind, OWNER, Picture,
    State, canonicalize, canonicalize_envelope,
};
pub use hlc::Rev;
pub use persona::{
    FieldClass, FieldRev, PersonaBody, PersonaDiff, PersonaField, ReplacedValue, field_class,
    persona_diff,
};
pub use rig::rig_package_id;

/// Why an entity or input was refused: what, and why. Her data holds no
/// secret, and neither does a message about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncError(String);

impl SyncError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub(crate) fn from_json(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SyncError {}

#[cfg(test)]
mod tests;
