//! The clock a side stamps its rows with, kept honest against the server.
//!
//! The log's clock persists and never goes back, so one row stamped while
//! the machine's clock ran ahead would keep every later row ahead too, and
//! the server refuses rows more than five minutes ahead of it
//! (`hlc::check_ahead`): the side could not sync until real time caught up.
//! Three rules keep that from happening, and undo it when it did:
//!
//! 1. **Corrected time.** The clock's physical input is the local time plus
//!    an offset measured against the server ([`offset`]) at every handshake
//!    and every `clock_ahead` answer. The caller keeps the offset and passes
//!    it in.
//! 2. **Clamp.** A write never moves the physical part past corrected time
//!    plus [`MAX_SKEW_MS`]: past that only the counter moves. A clock that
//!    is already past it stays where it is ([`poisoned`] says so) until
//!    [`repair`].
//! 3. **Repair.** Rows that never left the side may be stamped again;
//!    [`repair`] restamps only the unsynced rows from the first one that is
//!    too far ahead, in log order, and gives the clock to keep from then on.
//!
//! What a side does on `clock_ahead` (which carries `serverTime`): measure
//! the offset from it, repair the unsynced rows with that offset, store the
//! returned clock, and send again. A row that has left the side is never
//! restamped: the server took it, so it was not too far ahead.

use myriad_persona_sync::{
    Rev,
    hlc::{self, MAX_AHEAD_MS},
};
use serde::{Deserialize, Serialize};

use crate::LogError;

/// How far ahead of corrected time a stamp may go: the server's own limit.
pub const MAX_SKEW_MS: u64 = MAX_AHEAD_MS;

const MAX_COUNTER: u32 = 999_999;

/// The local clock's offset from the server, measured over one round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Offset {
    /// Add to local time to get the server's, in microseconds.
    pub offset: i64,
    /// Half the round trip: how wrong the offset can be, in microseconds.
    pub uncertainty: i64,
}

/// The offset from a request sent at local `sent`, answered with the
/// server's `server_time`, received at local `received` (all microseconds):
/// the server read its clock somewhere in between, taken as the middle.
pub fn offset(sent: i64, server_time: i64, received: i64) -> Result<Offset, LogError> {
    if received < sent {
        return Err(LogError::new("the answer came before the request"));
    }
    let middle = sent + (received - sent) / 2;
    Ok(Offset {
        offset: server_time - middle,
        uncertainty: (received - sent + 1) / 2,
    })
}

/// Local time corrected by `offset`, in milliseconds.
fn corrected_ms(now_local: i64, offset: i64) -> Result<u64, LogError> {
    let corrected = now_local
        .checked_add(offset)
        .filter(|micros| *micros >= 0)
        .ok_or_else(|| LogError::new("corrected time is out of range"))?;
    Ok((corrected / 1000) as u64)
}

fn ceiling(now_local: i64, offset: i64) -> Result<u64, LogError> {
    Ok(corrected_ms(now_local, offset)? + MAX_SKEW_MS)
}

/// Whether `last` is past corrected time plus [`MAX_SKEW_MS`]: whatever
/// this side stamps from it the server refuses, until [`repair`].
pub fn poisoned(last: &Rev, now_local: i64, offset: i64) -> Result<bool, LogError> {
    Ok(last.wall_ms > ceiling(now_local, offset)?)
}

fn rev(wall_ms: u64, counter: u32, node: &str) -> Result<Rev, LogError> {
    Rev::new(wall_ms, counter, node).map_err(|error| LogError::new(error.to_string()))
}

/// `wall` and the counter after `counter`, never borrowing a millisecond
/// past `ceiling`.
fn stamp(wall: u64, counter: Option<u32>, ceiling: u64, node: &str) -> Result<Rev, LogError> {
    match counter {
        None => rev(wall, 0, node),
        Some(counter) if counter < MAX_COUNTER => rev(wall, counter + 1, node),
        Some(_) if wall < ceiling => rev(wall + 1, 0, node),
        Some(_) => Err(LogError::new(
            "a million stamps in the last millisecond allowed",
        )),
    }
}

/// The stamp for a write at local `now_local` with `offset`, after `last`.
/// Always above `last`.
pub fn next(last: Option<&Rev>, now_local: i64, offset: i64, node: &str) -> Result<Rev, LogError> {
    hlc::check_node(node).map_err(|error| LogError::new(error.to_string()))?;
    let now = corrected_ms(now_local, offset)?;
    let top = ceiling(now_local, offset)?;
    match last {
        Some(last) if last.wall_ms >= now => stamp(
            last.wall_ms,
            Some(last.counter),
            top.max(last.wall_ms),
            node,
        ),
        _ => rev(now, 0, node),
    }
}

/// This side's clock after seeing `seen` from another side: above `last`
/// and `seen`, except that `seen` moves the physical part at most to the
/// ceiling. A row past it never gets here: [`crate::admit`] refuses its
/// batch.
pub fn observe(
    last: Option<&Rev>,
    seen: &Rev,
    now_local: i64,
    offset: i64,
    node: &str,
) -> Result<Rev, LogError> {
    hlc::check_node(node).map_err(|error| LogError::new(error.to_string()))?;
    let now = corrected_ms(now_local, offset)?;
    let top = ceiling(now_local, offset)?;
    let seen_wall = seen.wall_ms.min(top);
    let last_wall = last.map_or(0, |last| last.wall_ms);
    let wall = now.max(seen_wall).max(last_wall);
    let from_last = last
        .filter(|last| last.wall_ms == wall)
        .map(|last| last.counter);
    let from_seen = (seen.wall_ms == wall).then_some(seen.counter);
    stamp(wall, from_last.max(from_seen), top.max(wall), node)
}

/// One unsynced row and its stamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Stamped {
    pub id: String,
    pub hlc: Rev,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepairInput {
    /// The clock this side keeps.
    pub last: Rev,
    /// The newest revision known to have been taken elsewhere: the latest
    /// from the server, or the latest of this side's rows that left it.
    #[serde(default)]
    pub floor: Option<Rev>,
    /// Every row this side wrote that has not left it, any order.
    pub unsynced: Vec<Stamped>,
    pub now: i64,
    pub offset: i64,
    pub node: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Repaired {
    /// The clock to keep from now on.
    pub last: Rev,
    /// New stamps, only for the rows that needed one, in log order.
    pub restamped: Vec<Stamped>,
}

/// New stamps for the unsynced rows that are too far ahead (and every
/// unsynced row after the first of them, to keep their order), and the
/// clock to keep. Nothing changes when nothing is too far ahead and the
/// clock is not poisoned.
pub fn repair(input: RepairInput) -> Result<Repaired, LogError> {
    let RepairInput {
        last,
        floor,
        mut unsynced,
        now,
        offset,
        node,
    } = input;
    hlc::check_node(&node).map_err(|error| LogError::new(error.to_string()))?;
    let top = ceiling(now, offset)?;
    unsynced.sort_by(|a, b| (&a.hlc, &a.id).cmp(&(&b.hlc, &b.id)));
    let first_ahead = unsynced.iter().position(|row| row.hlc.wall_ms > top);
    let kept = &unsynced[..first_ahead.unwrap_or(unsynced.len())];
    // Restart from what is known to be fine: the floor, and the unsynced
    // rows that stay as they are. Never from the poisoned clock.
    let mut clock: Option<Rev> = floor
        .filter(|floor| floor.wall_ms <= top)
        .into_iter()
        .chain(kept.iter().map(|row| row.hlc.clone()))
        .max();
    let mut restamped = Vec::new();
    for row in first_ahead.map_or(&[][..], |at| &unsynced[at..]) {
        let stamp = next(clock.as_ref(), now, offset, &node)?;
        restamped.push(Stamped {
            id: row.id.clone(),
            hlc: stamp.clone(),
        });
        clock = Some(stamp);
    }
    let last = if last.wall_ms > top {
        match clock {
            Some(clock) => clock,
            None => next(None, now, offset, &node)?,
        }
    } else {
        clock.into_iter().chain([last]).max().expect("one at least")
    };
    Ok(Repaired { last, restamped })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: i64 = 60_000_000;
    const NOW: i64 = 1_760_000_000_000_000;

    fn at(wall_ms: u64) -> Rev {
        Rev::new(wall_ms, 0, "dev_mac").unwrap()
    }

    #[test]
    fn the_offset_is_the_middle_of_the_round_trip() {
        let measured = offset(NOW, NOW + 77 * MINUTE + 50_000, NOW + 100_000).unwrap();
        assert_eq!(measured.offset, 77 * MINUTE);
        assert_eq!(measured.uncertainty, 50_000);
        assert!(offset(NOW, NOW, NOW - 1).is_err());
    }

    #[test]
    fn a_fast_clock_stamps_corrected_time() {
        // The Mac runs 77 minutes ahead; the server says so.
        let fast = NOW + 77 * MINUTE;
        let stamp = next(None, fast, -77 * MINUTE, "dev_mac").unwrap();
        assert_eq!(stamp.wall_ms, (NOW / 1000) as u64);
        assert!(hlc::check_ahead(&stamp, NOW).is_ok());
    }

    #[test]
    fn seeing_a_row_from_the_future_moves_only_the_counter_past_the_ceiling() {
        let top = (NOW / 1000) as u64 + MAX_SKEW_MS;
        let far = Rev::new(top + 60_000, 4, "site").unwrap();
        let after = observe(None, &far, NOW, 0, "dev_mac").unwrap();
        assert_eq!(after.wall_ms, top);
        let again = next(Some(&after), NOW, 0, "dev_mac").unwrap();
        assert_eq!((again.wall_ms, again.counter), (top, after.counter + 1));
        // A clock already past the ceiling stays put: only repair helps.
        let stuck = at(top + 77 * 60_000);
        assert!(poisoned(&stuck, NOW, 0).unwrap());
        let stamp = next(Some(&stuck), NOW, 0, "dev_mac").unwrap();
        assert_eq!(stamp.wall_ms, stuck.wall_ms);
        // At the ceiling with the counter full, it refuses rather than pass it.
        let full = Rev::new(top, MAX_COUNTER, "dev_mac").unwrap();
        assert!(next(Some(&full), NOW, 0, "dev_mac").is_err());
    }

    #[test]
    fn repair_restamps_only_the_unsynced_rows_that_are_ahead() {
        let now_ms = (NOW / 1000) as u64;
        let ahead = now_ms + 77 * 60_000;
        let rows = vec![
            Stamped {
                id: "b".into(),
                hlc: at(ahead + 2),
            },
            Stamped {
                id: "ok".into(),
                hlc: at(now_ms - 10_000),
            },
            Stamped {
                id: "a".into(),
                hlc: at(ahead + 1),
            },
        ];
        let repaired = repair(RepairInput {
            last: at(ahead + 2),
            floor: Some(Rev::new(now_ms - 20_000, 0, "site").unwrap()),
            unsynced: rows,
            now: NOW,
            offset: 0,
            node: "dev_mac".into(),
        })
        .unwrap();
        let ids: Vec<&str> = repaired
            .restamped
            .iter()
            .map(|row| row.id.as_str())
            .collect();
        assert_eq!(ids, ["a", "b"]);
        assert!(repaired.restamped[0].hlc < repaired.restamped[1].hlc);
        assert!(
            repaired
                .restamped
                .iter()
                .all(|row| hlc::check_ahead(&row.hlc, NOW).is_ok())
        );
        assert_eq!(repaired.last, repaired.restamped[1].hlc);
        assert!(!poisoned(&repaired.last, NOW, 0).unwrap());

        // Nothing ahead, clock fine: nothing changes.
        let calm = repair(RepairInput {
            last: at(now_ms),
            floor: None,
            unsynced: vec![Stamped {
                id: "ok".into(),
                hlc: at(now_ms - 1),
            }],
            now: NOW,
            offset: 0,
            node: "dev_mac".into(),
        })
        .unwrap();
        assert!(calm.restamped.is_empty());
        assert_eq!(calm.last, at(now_ms));
    }
}
