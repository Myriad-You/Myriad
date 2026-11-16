//! The one alarm: she should be living here and nothing of her own life
//! ran for [`QUIET_SECS`]. Said to the site's owner once; said again only
//! after she has been well for [`WELL_SECS`].
//!
//! Why ten minutes (the owner's choice, 2026-11-16): while she lives, six
//! of her drivers are let through every 5 to 60 seconds each
//! (`persona::start`), and being let through comes before any database or
//! model, so a slow tick or a slow database leaves no gap. Ten minutes is
//! ten rounds of the slowest of those missed in a row, and one whole round
//! of her writing first, the slowest of what others can see of her; since
//! 2026-11-16 nothing pauses her on purpose while the site is her host. On
//! 2026-11-15 her mind was lent away 23 times, 5 to 46 minutes each: this
//! would have rung ten minutes into the first of those that lasted that
//! long, and once that day (`tests::the_fifteenth_rings_once`); the
//! shorter ones before it are on the page as times she stopped.

use serde::{Deserialize, Serialize};

/// Nothing of her own life for this long, while she should be living: said.
pub const QUIET_SECS: i64 = 10 * 60;
/// A stretch with nothing of hers this long is counted as one she stopped
/// for, and starts the wait for [`WELL_SECS`] over.
pub const GAP_SECS: i64 = 2 * 60;
/// Living this long without such a stretch after an alarm: she is well
/// again, and the next time is said anew.
pub const WELL_SECS: i64 = 60 * 60;
/// How long the stretches she stopped for are kept.
pub const GAPS_KEPT_SECS: i64 = 24 * 60 * 60;

/// What the watch keeps between looks.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Watch {
    /// Since when she should be living here, as this process saw it: none
    /// while she should not (her switch off, or the site not her host).
    pub should_since: Option<i64>,
    /// The alarm that stands: since when nothing of hers ran.
    pub alarmed_from: Option<i64>,
    /// After an alarm: since when she has been living without a gap.
    pub well_from: Option<i64>,
    /// A stretch she is stopped for now, from when.
    pub gap_from: Option<i64>,
    /// The stretches she stopped for, (from, to), the last day's.
    pub gaps: Vec<(i64, i64)>,
}

/// What one look sees.
#[derive(Debug, Clone, Copy)]
pub struct Seen {
    pub now: i64,
    /// When this process started her drivers: what ran before is not
    /// counted, so a restart is given the same ten minutes as any gap.
    pub started_at: i64,
    /// Her switch is on and the site is her host.
    pub should_live: bool,
    /// When one of her drivers was last let through.
    pub lived_at: Option<i64>,
}

/// What a look has to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Said {
    /// Nothing of hers since `from`.
    Stopped { from: i64 },
    /// She stopped at `from` and has been well since `since`.
    Back { from: i64, since: i64 },
}

impl Watch {
    /// Since when nothing of hers ran, as of `seen`.
    pub fn quiet_from(&self, seen: &Seen) -> i64 {
        seen.lived_at
            .unwrap_or(i64::MIN)
            .max(seen.started_at)
            .max(self.should_since.unwrap_or(i64::MIN))
    }

    /// One look. While she should not live here nothing is counted and an
    /// alarm that stood is let go: it is not one any more.
    pub fn look(&mut self, seen: Seen) -> Option<Said> {
        self.gaps.retain(|(_, to)| seen.now - *to < GAPS_KEPT_SECS);
        if !seen.should_live {
            *self = Self {
                gaps: std::mem::take(&mut self.gaps),
                ..Self::default()
            };
            return None;
        }
        let should_since = *self.should_since.get_or_insert(seen.now);
        let quiet_from = self.quiet_from(&seen).max(should_since);
        let quiet = seen.now - quiet_from;
        if quiet >= GAP_SECS {
            self.gap_from.get_or_insert(quiet_from);
            self.well_from = None;
        } else if let Some(from) = self.gap_from.take() {
            self.gaps.push((from, quiet_from));
        }
        match self.alarmed_from {
            None if quiet >= QUIET_SECS => {
                self.alarmed_from = Some(quiet_from);
                Some(Said::Stopped { from: quiet_from })
            }
            Some(from) if quiet < GAP_SECS => {
                let since = *self.well_from.get_or_insert(quiet_from.max(from));
                if seen.now - since >= WELL_SECS {
                    self.alarmed_from = None;
                    self.well_from = None;
                    Some(Said::Back { from, since })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// How many stretches she stopped for in the last day, and their
    /// seconds together, one going on now counted to `now`.
    pub fn gaps_of_the_day(&self, now: i64) -> (usize, i64) {
        let open = self.gap_from.map(|from| (from, now));
        let all = self.gaps.iter().copied().chain(open);
        all.fold((0, 0), |(count, seconds), (from, to)| {
            (count + 1, seconds + (to - from).max(0))
        })
    }
}
