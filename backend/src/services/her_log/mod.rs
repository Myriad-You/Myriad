//! Her log on the server: the relay between her host cores, and her one
//! lease (multi-device sync, step S6).
//!
//! Every host core (each PC, and this server) keeps its own append-only log
//! of her, one per audience; there is one her and one mind thinking at a
//! time. The server's part:
//!
//! - it keeps the logs it should ([`store`]): the owner's private talk and
//!   her own, which every host core keeps too, and each group's, which only
//!   the server keeps. What a row looks like, how a batch is checked and
//!   merged, what a side lacks and what forgetting does are all
//!   `myriad_persona_log`'s; nothing here decides them again;
//! - payloads are kept sealed, a key per object ([`keys`]); the server holds
//!   the keys (problem 5 of the sync plan), so forgetting an object deletes
//!   its key and its payload;
//! - sides swap version vectors and send each other what is missing, over
//!   her link's protocol-2 `log.*` frames ([`relay`]); a new host core's
//!   first full sync is a single-use HTTPS download ([`full`]);
//! - it issues the lease and decides how it moves ([`lease`]), by
//!   `myriad_persona_log::lease`;
//! - it deletes what is past its retention ([`retention`]); the owner's
//!   account carries the region its data is kept in from the first day
//!   (problem 13), empty until the region is decided.
//!
//! All of it is off unless `HER_LOG_ENABLED` is on ([`Settings`]): no client
//! speaks it yet, and the compliance questions of the sync plan (§6) are to
//! be settled before it serves anyone. While off, nothing here reads or
//! writes a table and the site behaves as it did.
//!
//! No key, token or secret is in a row, a frame, an answer or an error:
//! errors say what was refused, never with what.

// Nothing outside the tests reaches the relay until her link speaks
// protocol 2: the connection still speaks 1 and does not hand it frames.
#[cfg_attr(not(test), allow(dead_code))]
pub mod full;
#[cfg_attr(not(test), allow(dead_code))]
pub mod keys;
#[cfg_attr(not(test), allow(dead_code))]
pub mod lease;
#[cfg_attr(not(test), allow(dead_code))]
pub mod relay;
pub mod retention;
#[cfg_attr(not(test), allow(dead_code))]
pub mod store;

#[cfg(test)]
mod tests;

use std::fmt;
use std::sync::OnceLock;

use sea_orm::DbErr;

/// The server's node in her log and in the lease: the same as the site's
/// node in revisions.
pub const NODE: &str = myriad_persona_sync::hlc::SITE;

/// How her log on the server runs. Environment, read once at start: it is
/// deployment policy, not a site setting the admin changes from a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `HER_LOG_ENABLED`, default off.
    pub enabled: bool,
    /// `HER_LEASE_STOP_AFTER_SECS`, default 180: how long a holder cut off
    /// from the server, with the owner gone from its place, keeps thinking
    /// before it stops by itself. The server takes over by the same value
    /// and gives it to every holder (`stopAfter`).
    pub stop_after_micros: i64,
    /// `HER_LOG_RETENTION_DAYS`, default 30: how long her log is kept after
    /// the owner turned syncing off.
    pub retention_days: i64,
}

pub const DEFAULT_STOP_AFTER_SECS: i64 = 180;
pub const DEFAULT_RETENTION_DAYS: i64 = 30;

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            stop_after_micros: DEFAULT_STOP_AFTER_SECS * 1_000_000,
            retention_days: DEFAULT_RETENTION_DAYS,
        }
    }
}

impl Settings {
    pub fn from_env() -> Self {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let enabled = lookup("HER_LOG_ENABLED").is_some_and(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        });
        let stop_after_secs = lookup("HER_LEASE_STOP_AFTER_SECS")
            .and_then(|raw| raw.trim().parse::<i64>().ok())
            .filter(|secs| {
                let micros = secs.saturating_mul(1_000_000);
                let ok = myriad_persona_log::lease::stop_after(Some(micros)).is_ok();
                if !ok {
                    tracing::warn!(
                        secs,
                        "[her-log] HER_LEASE_STOP_AFTER_SECS must be 60 to 86400; using 180"
                    );
                }
                ok
            })
            .unwrap_or(DEFAULT_STOP_AFTER_SECS);
        let retention_days = lookup("HER_LOG_RETENTION_DAYS")
            .and_then(|raw| raw.trim().parse::<i64>().ok())
            .filter(|days| *days >= 1)
            .unwrap_or(DEFAULT_RETENTION_DAYS);
        Self {
            enabled,
            stop_after_micros: stop_after_secs * 1_000_000,
            retention_days,
        }
    }
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

/// The settings this process runs with.
pub fn settings() -> &'static Settings {
    SETTINGS.get_or_init(Settings::from_env)
}

/// Whether her log on the server is on.
pub fn enabled() -> bool {
    settings().enabled
}

/// Why something was not done. The message says what, never with what: no
/// key, token, payload or row content is ever in it.
#[derive(Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub enum Error {
    /// Her log on the server is off.
    Off,
    /// The input was not one this takes (a row, a frame, an audience).
    Refused(String),
    /// A batch with a clock too far ahead of the server's, refused whole.
    ClockAhead {
        ahead_ms: u64,
        server_time: i64,
    },
    /// A key could not be made, opened or used.
    Key,
    Db(DbErr),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off => f.write_str("her log on the server is off"),
            Self::Refused(what) => write!(f, "refused: {what}"),
            Self::ClockAhead { ahead_ms, .. } => {
                write!(f, "a row's clock is {ahead_ms} ms ahead of the server's")
            }
            Self::Key => f.write_str("a key could not be used"),
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

impl From<myriad_persona_log::LogError> for Error {
    fn from(error: myriad_persona_log::LogError) -> Self {
        Self::Refused(error.to_string())
    }
}

/// The server's time for her log, in microseconds: the site's one clock
/// (`crate::clock`), the same time `clock_ahead` compares device clocks
/// with. It is already corrected, so her log's clock takes no offset.
pub fn now() -> i64 {
    crate::clock::now_micros()
}
