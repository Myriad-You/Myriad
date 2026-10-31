//! Her log on the server: the relay between her host cores, and her one
//! lease (multi-device sync, step S6).
//!
//! Every host core (each PC, and this server) keeps its own append-only log
//! of her, one per audience; there is one her and one mind thinking at a
//! time. The server's part:
//!
//! - it keeps the logs it should ([`store`]): the owner's private talk and
//!   her own, which every host core keeps too, and each group's and
//!   anyone else's private talk, which only the server keeps
//!   (`myriad_persona_log::holder`). What a row looks like, how a batch is
//!   checked and merged, what a side lacks and what forgetting does are all
//!   `myriad_persona_log`'s; which bodies travel with their rows, and why
//!   the others do not, is `myriad_nodes::wire`'s; nothing here decides them
//!   again;
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
//! Her log is one per account ([`Owner`]): every table of rows, payloads,
//! keys, ledgers and counts is keyed by the account. What is still one for
//! the whole site: her lease (`agent_lease_grants`, `agent_lease_ends`: one
//! her, one mind), the server's clock (`agent_log_clock`, the server's one
//! node in every log), and the first-sync links (`agent_log_full_links`,
//! each for one device, and a device is one account's).
//!
//! All of it is off unless `HER_LOG_ENABLED` is on ([`Settings`]): no client
//! speaks it yet, and the compliance questions of the sync plan (§6) are to
//! be settled before it serves anyone. While off, nothing here reads or
//! writes a table and the site behaves as it did, with one exception: an
//! account's partner id ([`partner_id`]) is minted the first time a device
//! of it asks in its link's `hello` (`partner`), whether or not the site
//! keeps her log for it. That is the id alone; nothing of her is kept with
//! it.
//!
//! Not wired up yet: her link still speaks protocol 1, so nothing outside
//! the tests calls [`relay::take`], and no `log.*` frame reaches it from a
//! connection (nor is a first full download ever issued: only the relay
//! gives its link). Until it does, the log moves only in files: the export,
//! a move to a device, and taking such a file in.
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

use myriad_persona_log::{Core, Holder};
use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};

/// The server's node in her log and in the lease: the same as the site's
/// node in revisions.
pub const NODE: &str = myriad_persona_sync::hlc::SITE;

/// One account's side of her log: the account (`user`, the site's user id,
/// which keys every table of it) and the partner id it goes by. Node ids
/// are unique only per account and audience, so nothing of one account's
/// log is ever read for another's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub user: i32,
    /// The partner's one id (`partner_…`, [`partner_id`]): their private
    /// talk is `personal:<id>` on every side, and it is the `holder.owner`
    /// of the server and of the partner's computers.
    pub partner: String,
}

impl Owner {
    /// The partner's computers as a side of her log: they hold only the
    /// partner's private talk and hers.
    pub fn pc(&self) -> Holder {
        Holder {
            core: Core::Pc,
            owner: self.partner.clone(),
        }
    }

    /// The server as a side of this account's log: it holds every audience
    /// but another partner's private talk.
    pub fn server(&self) -> Holder {
        Holder {
            core: Core::Server,
            owner: self.partner.clone(),
        }
    }

    /// `personal:<partner id>`.
    pub fn personal(&self) -> String {
        format!("personal:{}", self.partner)
    }
}

/// The partner id of `user`'s account, minted the first time it is asked
/// for (`partner.mint`: the time and ten random bytes) and the same ever
/// after. The site is where every side of the account gets it (the link's
/// `welcome` `partner`); it is an id, not a credential.
pub async fn partner_id<C: ConnectionTrait>(db: &C, user: i32) -> Result<String, Error> {
    let read = || {
        Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT partner_id FROM agent_log_partners WHERE user_id = $1",
            [user.into()],
        )
    };
    if let Some(row) = db.query_one_raw(read()).await? {
        return Ok(row.try_get("", "partner_id")?);
    }
    let minted_at = crate::clock::now();
    let random: [u8; myriad_persona_log::partner::RANDOM_BYTES] = rand::random();
    let id = myriad_persona_log::partner::mint(minted_at.timestamp_millis(), &random)?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_log_partners (user_id, partner_id, minted_at) VALUES ($1, $2, $3) \
         ON CONFLICT (user_id) DO NOTHING",
        [user.into(), id.into(), minted_at.into()],
    ))
    .await?;
    // Read back: if another connection minted first, its id is the one.
    let row = db
        .query_one_raw(read())
        .await?
        .ok_or_else(|| Error::Refused("no partner id".into()))?;
    Ok(row.try_get("", "partner_id")?)
}

/// A fixed partner id for an account in tests, as if the site had minted it.
#[cfg(test)]
pub(crate) mod test_partner {
    use sea_orm::ConnectionTrait;

    /// One partner, minted on 2026-01-01.
    pub(crate) const PARTNER: &str = "partner_019b76daa80000112233445566778899";
    /// Another account's partner.
    pub(crate) const OTHER_PARTNER: &str = "partner_019b76daa80099887766554433221100";

    /// Gives `user`'s account the partner id `id`.
    pub(crate) async fn give<C: ConnectionTrait>(db: &C, user: i32, id: &str) {
        db.execute_unprepared(&format!(
            "INSERT INTO agent_log_partners (user_id, partner_id, minted_at) \
             VALUES ({user}, '{id}', NOW())"
        ))
        .await
        .unwrap();
    }
}

/// `user`'s account as a side of her log, its partner id minted if need be.
pub async fn owner<C: ConnectionTrait>(db: &C, user: i32) -> Result<Owner, Error> {
    Ok(Owner {
        user,
        partner: partner_id(db, user).await?,
    })
}

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

impl From<myriad_nodes::NodeError> for Error {
    fn from(error: myriad_nodes::NodeError) -> Self {
        Self::Refused(error.to_string())
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
