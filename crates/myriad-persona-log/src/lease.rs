//! The lease: which host core her one mind is on, numbered by an epoch that
//! only grows. Only the server issues it.
//!
//! It moves three ways, and no other:
//! 1. **Handover**: the holder gives it up (going to sleep, the owner left
//!    its place or showed up at another). It answers with its last count
//!    in each audience ([`Through`]); the target catches up to them before
//!    it thinks.
//! 2. **Takeover**: the server takes it when the holder has been out of
//!    reach for [`UNREACHABLE_MICROS`] and the owner's own position proves
//!    the owner is elsewhere. That is safe because a holder that is cut off
//!    and has had no owner in front of it for [`SELF_STOP_MICROS`] stops by
//!    itself ([`here`]), without needing the network to know.
//! 3. **Force**: the owner says "come here". The only way two of her can
//!    think at once (the old holder still running, offline); its rows from
//!    after the move are folded in and marked ([`standing`]).
//!
//! The times are first values, to be tuned once she is used for real. The
//! one the owner notices, how long a holder out of reach with the owner gone
//! keeps thinking, is the server's setting ([`MoveInput::stop_after`]): the
//! server decides takeovers with it and gives the same value to every holder
//! with the lease, which stops by it ([`HereInput::stop_after`]). Three
//! minutes when nothing says otherwise.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use myriad_persona_sync::{Rev, hlc};
use serde::{Deserialize, Serialize};

use crate::{
    LogError, Row,
    row::{Audience, Author, Replicate},
};

/// How long the holder must have been out of the server's reach before the
/// server may take over: three minutes.
pub const UNREACHABLE_MICROS: i64 = 3 * 60 * 1_000_000;
/// How long a holder cut off from the server, with no owner in its place,
/// keeps thinking before it stops by itself: three minutes.
pub const SELF_STOP_MICROS: i64 = 3 * 60 * 1_000_000;
/// How old the owner's position may be and still prove where they are:
/// three minutes. Older, they may have come back.
pub const OWNER_PLACE_FRESH_MICROS: i64 = 3 * 60 * 1_000_000;
/// The shortest a configured stop time may be: a minute. Shorter, a holder
/// would stop while the owner only looked away.
pub const MIN_STOP_AFTER_MICROS: i64 = 60 * 1_000_000;
/// The longest: a day.
pub const MAX_STOP_AFTER_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

/// The stop time to use: `configured`, or three minutes when absent. One
/// value for both waits ([`UNREACHABLE_MICROS`], [`SELF_STOP_MICROS`]), so
/// the server never takes over sooner than a holder stops.
pub fn stop_after(configured: Option<i64>) -> Result<i64, LogError> {
    let Some(micros) = configured else {
        return Ok(SELF_STOP_MICROS);
    };
    if !(MIN_STOP_AFTER_MICROS..=MAX_STOP_AFTER_MICROS).contains(&micros) {
        return Err(LogError::new(format!(
            "stopAfter must be {MIN_STOP_AFTER_MICROS} to {MAX_STOP_AFTER_MICROS} microseconds"
        )));
    }
    Ok(micros)
}

/// How a grant came about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    /// The first one, when she first lives anywhere.
    First,
    Handover,
    Takeover,
    Force,
}

/// One issue of the lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Grant {
    pub epoch: u64,
    /// The host core that holds it (a clock node).
    pub holder: String,
    /// The server's clock when it issued it.
    pub issued: Rev,
    pub how: How,
    /// For a handover: the previous holder's last shared count in each
    /// audience when it gave the lease up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through: Option<Through>,
}

/// A side's last shared count in each audience it wrote in: `{audience:
/// dseq}`. Counts are per audience ([`Row::dseq`]), so one number cannot
/// say it. An audience it wrote nothing in is left out, never 0.
pub type Through = BTreeMap<String, u64>;

/// Checks a [`Through`]: real audiences, counts from 1.
pub fn check_through(through: &Through) -> Result<(), LogError> {
    for (audience, &count) in through {
        Audience::parse(audience)?;
        if count == 0 {
            return Err(LogError::new(
                "`through` leaves out an audience with nothing in it",
            ));
        }
    }
    Ok(())
}

impl Grant {
    pub fn check(&self) -> Result<(), LogError> {
        if self.epoch == 0 {
            return Err(LogError::new("epochs start at 1"));
        }
        hlc::check_node(&self.holder).map_err(|error| LogError::new(error.to_string()))?;
        if let Some(through) = &self.through {
            if self.how != How::Handover {
                return Err(LogError::new("only a handover carries `through`"));
            }
            check_through(through)?;
        }
        Ok(())
    }

    fn rank(&self) -> (u64, std::cmp::Reverse<&Rev>, std::cmp::Reverse<&str>) {
        (
            self.epoch,
            std::cmp::Reverse(&self.issued),
            std::cmp::Reverse(self.holder.as_str()),
        )
    }
}

/// Which of two grants stands: the higher epoch. Two grants of one epoch
/// should never exist (only the server issues, one at a time); if they do,
/// the one issued first stands, then the lower holder, so every side picks
/// the same one.
pub fn newer<'a>(a: &'a Grant, b: &'a Grant) -> &'a Grant {
    match a.rank().cmp(&b.rank()) {
        Ordering::Less => b,
        _ => a,
    }
}

/// Where the owner was last seen, by a device that recognized the place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnerPlace {
    pub place: String,
    /// Since when they have been there (first seen there, this time).
    pub since: i64,
    /// When they were last seen there.
    pub seen_at: i64,
}

/// A request to move the lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// The holder gives it to `to`.
    Handover {
        from: String,
        /// The epoch `from` holds.
        epoch: u64,
        to: String,
        /// `from`'s last shared count in each audience.
        #[serde(default)]
        through: Option<Through>,
    },
    /// The server (`server`, its node) takes it.
    Takeover { server: String },
    /// The owner brings her to `to`.
    Force { to: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MoveInput {
    /// The grant that stands now.
    pub lease: Grant,
    pub request: Request,
    /// The server's clock.
    pub now: i64,
    /// The server's revision for a new grant (after the current one's).
    pub issued: Rev,
    /// The last moment the holder may still have believed itself in reach:
    /// when the server last heard from it, plus the time the holder takes to
    /// notice it is cut off. Its self-stop counts from there.
    #[serde(default)]
    pub holder_last_seen: Option<i64>,
    /// The place the holder is in, when it knows.
    #[serde(default)]
    pub holder_place: Option<String>,
    #[serde(default)]
    pub owner_place: Option<OwnerPlace>,
    /// How long the holder must be out of reach, and the owner away from
    /// it, before it has stopped by itself: the server's setting, the value
    /// it gives the holders. Three minutes when absent ([`stop_after`]).
    #[serde(default)]
    pub stop_after: Option<i64>,
}

/// Why a move was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    /// The target holds it already.
    AlreadyHolder,
    /// A handover from a side that does not hold it.
    NotHolder,
    /// A handover of an epoch that no longer stands.
    StaleEpoch,
    /// The server heard from the holder too recently.
    HolderRecent,
    /// Nothing says where the holder is.
    HolderPlaceUnknown,
    /// Nothing says where the owner is.
    OwnerPlaceUnknown,
    /// The owner is where the holder is: she is there, reached locally.
    SamePlace,
    /// The owner's position is too old to prove anything.
    OwnerPlaceStale,
    /// The owner left too recently for the holder to have stopped.
    OwnerLeftRecently,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Moved {
    Granted(Grant),
    Refused(Refusal),
}

/// Whether the server moves the lease as asked, and the new grant.
pub fn decide(input: MoveInput) -> Result<Moved, LogError> {
    let MoveInput {
        lease,
        request,
        now,
        issued,
        holder_last_seen,
        holder_place,
        owner_place,
        stop_after: configured,
    } = input;
    lease.check()?;
    let stop = stop_after(configured)?;
    if issued <= lease.issued {
        return Err(LogError::new(
            "a new grant is issued after the one standing",
        ));
    }
    let grant = |holder: &str, how, through| {
        let grant = Grant {
            epoch: lease.epoch + 1,
            holder: holder.to_string(),
            issued: issued.clone(),
            how,
            through,
        };
        grant.check().map(|()| Moved::Granted(grant))
    };
    let refused = |refusal| Ok(Moved::Refused(refusal));
    match request {
        Request::Handover {
            from,
            epoch,
            to,
            through,
        } => {
            if from != lease.holder {
                return refused(Refusal::NotHolder);
            }
            if epoch != lease.epoch {
                return refused(Refusal::StaleEpoch);
            }
            if to == lease.holder {
                return refused(Refusal::AlreadyHolder);
            }
            grant(&to, How::Handover, through)
        }
        Request::Force { to } => {
            if to == lease.holder {
                return refused(Refusal::AlreadyHolder);
            }
            grant(&to, How::Force, None)
        }
        Request::Takeover { server } => {
            if server == lease.holder {
                return refused(Refusal::AlreadyHolder);
            }
            let last_seen =
                holder_last_seen.ok_or_else(|| LogError::new("a takeover needs holderLastSeen"))?;
            if now - last_seen < stop {
                return refused(Refusal::HolderRecent);
            }
            let Some(holder_place) = holder_place else {
                return refused(Refusal::HolderPlaceUnknown);
            };
            let Some(owner) = owner_place else {
                return refused(Refusal::OwnerPlaceUnknown);
            };
            if owner.place == holder_place {
                return refused(Refusal::SamePlace);
            }
            if now - owner.seen_at > OWNER_PLACE_FRESH_MICROS {
                return refused(Refusal::OwnerPlaceStale);
            }
            // The holder stops by itself once it has been cut off and had
            // no owner in front of it for the stop time. The owner was not
            // in front of it from `since` on, and it was cut off from
            // `last_seen` on: so it has stopped by the later of the two
            // plus that time.
            if now < last_seen.max(owner.since) + stop {
                return refused(Refusal::OwnerLeftRecently);
            }
            grant(&server, How::Takeover, None)
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HereInput {
    /// This side's node.
    pub me: String,
    /// The newest grant this side knows. A side that never met the server
    /// holds its own first grant.
    pub lease: Grant,
    pub now: i64,
    /// Since when this side has been cut off from the server; absent while
    /// it is connected.
    #[serde(default)]
    pub disconnected_since: Option<i64>,
    /// The last time the owner was in this side's place: input on it, or
    /// one of the owner's devices reaching it on the local network.
    #[serde(default)]
    pub owner_here_at: Option<i64>,
    /// The stop time the server gave with the lease; three minutes when
    /// absent ([`stop_after`]).
    #[serde(default)]
    pub stop_after: Option<i64>,
}

/// What her mind does on this side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum Here {
    /// She thinks here: the holder, in reach or with the owner present.
    Thinking,
    /// The holder stopped by itself at `stoppedAt`: she no longer thinks or
    /// writes her rows here; it still takes the owner's rows and finishes
    /// work in hand. Back in reach with the lease unchanged, she thinks
    /// again.
    #[serde(rename_all = "camelCase")]
    Stopped { stopped_at: i64 },
    /// Another side holds the lease (or a newer epoch moved it away): this
    /// side is a child.
    Child,
}

pub fn here(input: HereInput) -> Result<Here, LogError> {
    input.lease.check()?;
    if input.lease.holder != input.me {
        return Ok(Here::Child);
    }
    let Some(cut_off) = input.disconnected_since else {
        return Ok(Here::Thinking);
    };
    let from = input
        .owner_here_at
        .map_or(cut_off, |seen| seen.max(cut_off));
    let stopped_at = from + stop_after(input.stop_after)?;
    Ok(if input.now >= stopped_at {
        Here::Stopped { stopped_at }
    } else {
        Here::Thinking
    })
}

/// Where one of her rows stands against the lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Standing {
    pub id: String,
    pub origin: String,
    pub epoch: u64,
    pub standing: Stand,
    /// For [`Stand::Concurrent`]: the forced epoch it overlaps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stand {
    /// Written while its epoch stood, or before the move that ended it
    /// (rows that arrive late after a takeover are her own memory catching
    /// up, not another her).
    Normal,
    /// Written by the old holder after the owner forced a move: she was
    /// thinking in two places for a while. Kept and marked.
    Concurrent,
    /// Not written by the holder of its epoch, or written after it handed
    /// the lease over: a lease bug, to look into.
    NotHolder,
    AfterHandover,
}

/// Where `row` stands against `grants` (any order, any subset of the
/// history). The owner's rows, and her rows of an epoch not in `grants`,
/// are normal.
pub fn standing(row: &Row, grants: &[Grant]) -> Stand {
    let (Author::Her, Some(epoch)) = (row.author, row.epoch) else {
        return Stand::Normal;
    };
    let Some(own) = issue(grants, epoch) else {
        return Stand::Normal;
    };
    if own.holder != row.origin {
        return Stand::NotHolder;
    }
    let Some(next) = next_epoch(epoch, grants).and_then(|next| issue(grants, next)) else {
        return Stand::Normal;
    };
    let after_issue = row.hlc > next.issued;
    match next.how {
        How::Handover => {
            // An audience the handover leaves out had none of the old
            // holder's rows in it: every one there came after.
            let after = match (row.replicate, row.dseq, &next.through) {
                (Replicate::Shared, Some(dseq), Some(through)) => {
                    dseq > through.get(&row.audience).copied().unwrap_or(0)
                }
                _ => after_issue,
            };
            if after {
                Stand::AfterHandover
            } else {
                Stand::Normal
            }
        }
        How::Force if after_issue => Stand::Concurrent,
        How::Force | How::Takeover | How::First => Stand::Normal,
    }
}

/// Every one of `rows` that does not stand normal, in log order.
pub fn standings(rows: &[Row], grants: &[Grant]) -> Vec<Standing> {
    rows.iter()
        .filter_map(|row| {
            let stand = standing(row, grants);
            (stand != Stand::Normal).then(|| Standing {
                id: row.id.clone(),
                origin: row.origin.clone(),
                epoch: row.epoch.unwrap_or(0),
                standing: stand,
                with: (stand == Stand::Concurrent)
                    .then(|| row.epoch.and_then(|epoch| next_epoch(epoch, grants)))
                    .flatten(),
            })
        })
        .collect()
}

/// The grant of `epoch` that stands, when `grants` has one.
fn issue(grants: &[Grant], epoch: u64) -> Option<&Grant> {
    grants
        .iter()
        .filter(|grant| grant.epoch == epoch)
        .reduce(|a, b| newer(a, b))
}

/// The first epoch after `epoch` in `grants`.
fn next_epoch(epoch: u64, grants: &[Grant]) -> Option<u64> {
    grants
        .iter()
        .map(|grant| grant.epoch)
        .filter(|&next| next > epoch)
        .min()
}
