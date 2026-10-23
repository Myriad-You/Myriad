//! The clock revisions are stamped with: a hybrid logical clock.
//!
//! A revision is `<wall ms, 15 digits>-<counter, 6 digits>-<node>`. The
//! fixed widths make byte order the order of time, then counter, then node,
//! so two revisions compare as strings. The wall part follows the writer's
//! clock; the counter orders writes within one millisecond and absorbs a
//! clock that stepped back. Every side keeps the last revision it issued or
//! saw ([`next`], [`observe`]) and never issues one below it.
//!
//! A clock running ahead is the one thing this cannot absorb: a revision
//! stamped in the future wins every edit made after it until real time
//! catches up, and every side that observes it is pulled into the future
//! too. So the site takes no revision more than [`MAX_AHEAD_MS`] ahead of
//! its own clock ([`check_ahead`]); the device is told its clock is wrong
//! (`clock_ahead`, with the site's time) and syncs again once it is right.
//! The site's clock there is its one clock: real time when the site has a
//! trusted measurement of how far its host's clock is off (`serverClock`),
//! so a site whose host is behind does not refuse a device that is right;
//! the limit stays the same.
//!
//! Where the site checks: the `X-Sync-Clock` header every device request
//! may carry (the revision the device's next write would get, `next`
//! without keeping it), required on `hello` and `role`; and, once pushing
//! exists, every revision in a push (each entity's `rev` and each persona
//! field's `rev`). A push with any revision too far ahead is refused whole,
//! before anything is written or observed, so nothing from the future lands
//! and the site's clock is not dragged along.
//!
//! What a device does on `clock_ahead`: stop syncing and say so, keeping
//! its outbox (the premise of syncing has not gone; only the clock is
//! wrong). Once its clock is right, what it stamped while ahead is still
//! from the future, and the site never took any of it: it sets its clock
//! back to the latest revision it saw from the site, stamps each pending
//! write again with [`next`] in outbox order, and pushes. Before pushing it
//! can check itself with [`check_ahead`] against the site's time as `hello`
//! last gave it (`serverTime`, moved on by the time since).

use std::{cmp::Ordering, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::SyncError;

const WALL_DIGITS: usize = 15;
const COUNTER_DIGITS: usize = 6;
const MAX_WALL_MS: u64 = 999_999_999_999_999;
const MAX_COUNTER: u32 = 999_999;
/// Longest node: `dev_` plus a simple uuid is 36; `loc_` plus a hyphenated
/// uuid is 40.
pub const MAX_NODE_CHARS: usize = 64;
/// The site's node.
pub const SITE: &str = "site";
/// How far ahead of the site's clock a revision may be: five minutes.
pub const MAX_AHEAD_MS: u64 = 5 * 60 * 1000;

/// One revision.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rev {
    pub wall_ms: u64,
    pub counter: u32,
    pub node: String,
}

impl Rev {
    pub fn new(wall_ms: u64, counter: u32, node: &str) -> Result<Self, SyncError> {
        if wall_ms > MAX_WALL_MS {
            return Err(SyncError::new(format!(
                "wall time {wall_ms} is out of range"
            )));
        }
        if counter > MAX_COUNTER {
            return Err(SyncError::new(format!("counter {counter} is out of range")));
        }
        check_node(node)?;
        Ok(Self {
            wall_ms,
            counter,
            node: node.to_string(),
        })
    }
}

/// A node names one side: `site`, a device (`dev_…`) or an unconnected app
/// (`loc_…`). Printable ASCII without spaces, so a revision stays one token.
pub fn check_node(node: &str) -> Result<(), SyncError> {
    if node.is_empty() || node.len() > MAX_NODE_CHARS {
        return Err(SyncError::new(format!(
            "node must be 1 to {MAX_NODE_CHARS} characters"
        )));
    }
    if !node.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(SyncError::new(format!(
            "node {node:?} is not printable ASCII"
        )));
    }
    Ok(())
}

impl fmt::Display for Rev {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:0wall$}-{:0counter$}-{}",
            self.wall_ms,
            self.counter,
            self.node,
            wall = WALL_DIGITS,
            counter = COUNTER_DIGITS
        )
    }
}

impl FromStr for Rev {
    type Err = SyncError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let bad = || SyncError::new(format!("revision {raw:?} is not <wall>-<counter>-<node>"));
        let bytes = raw.as_bytes();
        let node_at = WALL_DIGITS + 1 + COUNTER_DIGITS + 1;
        if bytes.len() <= node_at
            || bytes[WALL_DIGITS] != b'-'
            || bytes[WALL_DIGITS + 1 + COUNTER_DIGITS] != b'-'
        {
            return Err(bad());
        }
        let digits = |part: &[u8]| part.iter().all(u8::is_ascii_digit);
        let wall = &bytes[..WALL_DIGITS];
        let counter = &bytes[WALL_DIGITS + 1..WALL_DIGITS + 1 + COUNTER_DIGITS];
        if !digits(wall) || !digits(counter) {
            return Err(bad());
        }
        let wall_ms = raw[..WALL_DIGITS].parse().map_err(|_| bad())?;
        let counter = raw[WALL_DIGITS + 1..WALL_DIGITS + 1 + COUNTER_DIGITS]
            .parse()
            .map_err(|_| bad())?;
        Rev::new(wall_ms, counter, &raw[node_at..])
    }
}

impl Ord for Rev {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.wall_ms, self.counter, self.node.as_bytes()).cmp(&(
            other.wall_ms,
            other.counter,
            other.node.as_bytes(),
        ))
    }
}

impl PartialOrd for Rev {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Serialize for Rev {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Rev {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

fn wall_of(now_micros: i64) -> Result<u64, SyncError> {
    if now_micros < 0 {
        return Err(SyncError::new(format!("time {now_micros} is before 1970")));
    }
    let wall = (now_micros / 1000) as u64;
    if wall > MAX_WALL_MS {
        return Err(SyncError::new(format!("time {now_micros} is out of range")));
    }
    Ok(wall)
}

/// A wall time and counter, carried past the counter's last value.
fn stamped(wall_ms: u64, counter: u64, node: &str) -> Result<Rev, SyncError> {
    if counter > u64::from(MAX_COUNTER) {
        // A thousand writes in one millisecond: borrow the next one.
        return Rev::new(wall_ms + 1, 0, node);
    }
    Rev::new(wall_ms, counter as u32, node)
}

/// The revision for a write by `node` at `now_micros`, after `last` (the
/// last revision this side issued or saw). Always above `last`.
pub fn next(last: Option<&Rev>, now_micros: i64, node: &str) -> Result<Rev, SyncError> {
    check_node(node)?;
    let now = wall_of(now_micros)?;
    match last {
        Some(last) if last.wall_ms >= now => {
            stamped(last.wall_ms, u64::from(last.counter) + 1, node)
        }
        _ => Rev::new(now, 0, node),
    }
}

/// This side's clock after seeing `seen` from another side at `now_micros`:
/// above both `last` and `seen`, so the next write here orders after it.
pub fn observe(
    last: Option<&Rev>,
    seen: &Rev,
    now_micros: i64,
    node: &str,
) -> Result<Rev, SyncError> {
    check_node(node)?;
    let now = wall_of(now_micros)?;
    let last_wall = last.map(|last| last.wall_ms);
    let wall = now.max(seen.wall_ms).max(last_wall.unwrap_or(0));
    let from_last = last
        .filter(|last| last.wall_ms == wall)
        .map(|last| u64::from(last.counter));
    let from_seen = (seen.wall_ms == wall).then_some(u64::from(seen.counter));
    match from_last.max(from_seen) {
        Some(counter) => stamped(wall, counter + 1, node),
        None => Rev::new(wall, 0, node),
    }
}

/// A revision stamped too far ahead of the clock it was checked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ahead {
    /// How far ahead, in milliseconds.
    pub ahead_ms: u64,
}

impl fmt::Display for Ahead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the clock is {} ms ahead (at most {MAX_AHEAD_MS} ms)",
            self.ahead_ms
        )
    }
}

impl std::error::Error for Ahead {}

/// How far `rev` is ahead of `now_micros` (the site's clock), in
/// milliseconds; zero when it is not.
pub fn ahead_ms(rev: &Rev, now_micros: i64) -> u64 {
    rev.wall_ms
        .saturating_sub((now_micros.max(0) / 1000) as u64)
}

/// Whether the site takes `rev` at `now_micros`: at most [`MAX_AHEAD_MS`]
/// ahead of it. A revision behind the clock is always fine.
pub fn check_ahead(rev: &Rev, now_micros: i64) -> Result<(), Ahead> {
    let ahead_ms = ahead_ms(rev, now_micros);
    if ahead_ms > MAX_AHEAD_MS {
        Err(Ahead { ahead_ms })
    } else {
        Ok(())
    }
}

/// The revision of a row written before revisions existed: its last change
/// in milliseconds, counter 0, and the side that wrote it (the site when
/// unknown). Computed when read, never stored.
pub fn legacy(updated_at_micros: i64, origin: Option<&str>) -> Result<Rev, SyncError> {
    let node = origin.filter(|origin| !origin.is_empty()).unwrap_or(SITE);
    Rev::new(wall_of(updated_at_micros.max(0))?, 0, node)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rev(raw: &str) -> Rev {
        raw.parse().unwrap()
    }

    #[test]
    fn a_revision_prints_and_parses_back() {
        let one = Rev::new(1_760_000_000_123, 2, "dev_8c1e").unwrap();
        assert_eq!(one.to_string(), "001760000000123-000002-dev_8c1e");
        assert_eq!(rev("001760000000123-000002-dev_8c1e"), one);
        // A node may hold hyphens: only the first two separate.
        assert_eq!(
            rev("000000000000001-000000-loc_1f0c-aa").node,
            "loc_1f0c-aa"
        );
        for bad in [
            "",
            "1760000000123-2-dev",
            "001760000000123-000002-",
            "001760000000123_000002-dev",
            "00176000000012x-000002-dev",
            "001760000000123-000002-has space",
        ] {
            assert!(bad.parse::<Rev>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn string_order_is_time_then_counter_then_node() {
        let mut revs = [
            rev("001760000000124-000000-a"),
            rev("001760000000123-000001-a"),
            rev("001760000000123-000000-b"),
            rev("001760000000123-000000-a"),
        ];
        let mut strings: Vec<String> = revs.iter().map(Rev::to_string).collect();
        revs.sort();
        strings.sort();
        assert_eq!(revs.iter().map(Rev::to_string).collect::<Vec<_>>(), strings);
    }

    #[test]
    fn next_moves_forward_even_when_the_clock_does_not() {
        let first = next(None, 1_760_000_000_123_456, "site").unwrap();
        assert_eq!(first.to_string(), "001760000000123-000000-site");
        let same_ms = next(Some(&first), 1_760_000_000_123_999, "site").unwrap();
        assert_eq!(same_ms.to_string(), "001760000000123-000001-site");
        let stepped_back = next(Some(&same_ms), 1_700_000_000_000_000, "site").unwrap();
        assert_eq!(stepped_back.to_string(), "001760000000123-000002-site");
        let later = next(Some(&stepped_back), 1_760_000_000_200_000, "site").unwrap();
        assert_eq!(later.to_string(), "001760000000200-000000-site");
        let full = Rev::new(5, MAX_COUNTER, "site").unwrap();
        assert_eq!(
            next(Some(&full), 0, "site").unwrap().to_string(),
            "000000000000006-000000-site"
        );
        assert!(next(None, -1, "site").is_err());
        assert!(next(None, 0, "").is_err());
    }

    #[test]
    fn observing_a_later_revision_puts_the_next_write_after_it() {
        let mine = rev("001760000000100-000003-site");
        let theirs = rev("001760000000500-000007-dev_a");
        let after = observe(Some(&mine), &theirs, 1_760_000_000_200_000, "site").unwrap();
        assert_eq!(after.to_string(), "001760000000500-000008-site");
        assert!(next(Some(&after), 1_760_000_000_200_000, "site").unwrap() > theirs);
        // Same millisecond on both: past both counters.
        let tied = observe(
            Some(&rev("001760000000500-000009-site")),
            &theirs,
            0,
            "site",
        )
        .unwrap();
        assert_eq!(tied.to_string(), "001760000000500-000010-site");
        // Both behind the clock: the clock wins, counter 0.
        let behind = observe(Some(&mine), &theirs, 1_760_000_001_000_000, "site").unwrap();
        assert_eq!(behind.to_string(), "001760000001000-000000-site");
        // Nothing seen before.
        let fresh = observe(None, &theirs, 0, "loc_1").unwrap();
        assert_eq!(fresh.to_string(), "001760000000500-000008-loc_1");
    }

    #[test]
    fn a_revision_more_than_five_minutes_ahead_is_refused() {
        let now = 1_760_000_000_000_000_i64;
        let at = |ms: u64| Rev::new(ms, 0, "dev_a").unwrap();
        let now_ms = 1_760_000_000_000;
        assert_eq!(check_ahead(&at(now_ms - 3_600_000), now), Ok(()));
        assert_eq!(check_ahead(&at(now_ms), now), Ok(()));
        assert_eq!(check_ahead(&at(now_ms + MAX_AHEAD_MS), now), Ok(()));
        assert_eq!(
            check_ahead(&at(now_ms + MAX_AHEAD_MS + 1), now),
            Err(Ahead {
                ahead_ms: MAX_AHEAD_MS + 1
            })
        );
        assert_eq!(ahead_ms(&at(now_ms - 1), now), 0);
        assert_eq!(ahead_ms(&at(5), -1), 5);
        // A counter is no clock: the wall time alone counts.
        let busy = Rev::new(now_ms + MAX_AHEAD_MS, 999_999, "dev_a").unwrap();
        assert!(check_ahead(&busy, now).is_ok());
    }

    #[test]
    fn a_legacy_row_takes_its_last_change() {
        assert_eq!(
            legacy(1_760_000_000_123_456, None).unwrap().to_string(),
            "001760000000123-000000-site"
        );
        assert_eq!(
            legacy(1_760_000_000_123_456, Some("dev_x"))
                .unwrap()
                .to_string(),
            "001760000000123-000000-dev_x"
        );
    }
}
