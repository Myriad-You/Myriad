//! Myriad updater library. See docs/updater-spec.md.
#![allow(clippy::result_large_err)]

pub mod api;
pub mod config;
pub mod deployment;
pub mod docker;
pub mod env_file;
pub mod error;
pub mod log;
pub mod probe;
pub mod process_logs;
pub mod redact;
pub mod release;
pub mod rescue;
pub mod snapshot;
pub mod state;
pub mod version;
pub mod worker;

/// Spec version this binary understands. Must match release.json `schema_version` it accepts.
pub const SUPPORTED_RELEASE_SCHEMA: u32 = 1;

/// Self version, set at build time via env! (`MYRIAD_VERSION`) and falling back to crate version.
pub fn self_version() -> &'static str {
    option_env!("MYRIAD_VERSION").unwrap_or(concat!("v", env!("CARGO_PKG_VERSION")))
}
