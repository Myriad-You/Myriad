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
//!
//! The host itself stopping is not her stopping. The site runs on a machine
//! that sleeps: the drivers' clocks stand still while it does and the wall
//! clock does not, so the first look after it wakes would see an hour with
//! nothing of hers. A look that comes [`SLEPT_SECS`] or more after the one
//! before it knows the host (or this process) was not running in between:
//! that stretch is kept apart, as one the host stopped for, and her ten
//! minutes start over from the look that found it.
//!
//! Two troubles short of her stopping are said the same way, each once and
//! once more when it has been gone for [`WELL_SECS`] ([`Held`]): one of her
//! drivers stuck while the others run ([`stalled`]), and her model not
//! answering her at all ([`UNANSWERED_IN_A_ROW`]). An answer that cannot be
//! read is an answer: that trouble is the model's, and its own counts show.
//! A stuck driver is stuck in one process, so one stuck after a restart is
//! said, whatever the last process had said.

use serde::{Deserialize, Serialize};

/// Nothing of her own life for this long, while she should be living: said.
pub const QUIET_SECS: i64 = 10 * 60;
/// A stretch with nothing of hers this long is counted as one she stopped
/// for, and starts the wait for [`WELL_SECS`] over.
pub const GAP_SECS: i64 = 2 * 60;
/// Living this long without such a stretch after an alarm: she is well
/// again, and the next time is said anew.
pub const WELL_SECS: i64 = 60 * 60;
/// Looks come every `LOOK_SECS` (15): one this long after the last means
/// nothing ran in between, the watch included (the host slept, or this
/// process was down). Four looks missed in a row; a slow tick does not
/// hold the look's own clock back that long.
pub const SLEPT_SECS: i64 = 60;
/// How long the stretches she stopped for are kept.
pub const GAPS_KEPT_SECS: i64 = 24 * 60 * 60;
/// Her calls to her model in a row with no answer at all, held for at
/// least [`QUIET_SECS`]: said. Ten in a row is not one slow minute: her
/// chat alone asks several times a turn, her own life every few minutes.
pub const UNANSWERED_IN_A_ROW: u64 = 10;

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
    /// Of the time since `well_from`, the seconds the host was stopped:
    /// not time she was well for.
    pub well_slept: i64,
    /// A stretch she is stopped for now, from when.
    pub gap_from: Option<i64>,
    /// The stretches she stopped for, (from, to), the last day's.
    pub gaps: Vec<(i64, i64)>,
    /// When the watch last looked.
    pub looked_at: Option<i64>,
    /// The look that found the host had stopped: what came before it is not
    /// counted against her.
    pub woke_at: Option<i64>,
    /// The stretches the host itself stopped for (asleep, or this process
    /// down), (from, to), the last day's. Not hers.
    pub host_stops: Vec<(i64, i64)>,
    /// One of her drivers stuck while the others run.
    pub stalled: Held,
    /// Her model not answering her.
    pub unanswered: Held,
}

/// A trouble said once while it holds, and once more when it has been gone
/// for [`WELL_SECS`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Held {
    /// Since when it holds, as said.
    pub from: Option<i64>,
    /// Since when it has been gone, after it was said.
    pub well_from: Option<i64>,
    /// Said by a process that is gone, and not seen in this one.
    #[serde(skip)]
    pub before: bool,
}

/// What a trouble has to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    Began { from: i64 },
    Ended { from: i64, since: i64 },
}

/// Which trouble, and what it has to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trouble {
    Stalled(Turn),
    Unanswered(Turn),
}

impl Held {
    /// As a new process takes it over, for a trouble that goes with its
    /// process (a tick that hangs ends with it): the next time it holds is
    /// another, and is said. Not seen again for [`WELL_SECS`], it ends as
    /// any does.
    pub fn said_before(self) -> Self {
        Self {
            before: self.from.is_some(),
            ..self
        }
    }

    /// One look: `holds_from` is since when the trouble has held, none when
    /// it does not hold now.
    pub fn look(&mut self, now: i64, holds_from: Option<i64>) -> Option<Turn> {
        match (self.from, holds_from) {
            (Some(_), Some(_)) if !self.before => {
                self.well_from = None;
                None
            }
            (_, Some(from)) => {
                *self = Self {
                    from: Some(from),
                    ..Self::default()
                };
                Some(Turn::Began { from })
            }
            (Some(from), None) => {
                let since = *self.well_from.get_or_insert(now);
                if now - since >= WELL_SECS {
                    *self = Self::default();
                    Some(Turn::Ended { from, since })
                } else {
                    None
                }
            }
            (None, None) => None,
        }
    }
}

/// Of her drivers that tick on their own (`every` seconds, last asked to be
/// let through at `asked_at`), how many are stuck as of `now`, and since
/// when the longest stuck one has been: not asked for three of its own
/// rounds, and never less than [`QUIET_SECS`]. A driver's ticks run one
/// after another, so one stuck in a tick is one that does not ask. `base` is
/// when the watch could first have seen it run (this process started, she
/// came to live here, the host woke).
pub fn stalled(
    beats: impl IntoIterator<Item = (u64, i64)>,
    base: i64,
    now: i64,
) -> (usize, Option<i64>) {
    beats
        .into_iter()
        .filter_map(|(every, asked_at)| {
            let from = asked_at.max(base);
            let allowed = (3 * every as i64).max(QUIET_SECS);
            (now - from >= allowed).then_some(from)
        })
        .fold((0, None), |(count, earliest): (usize, Option<i64>), from| {
            (count + 1, Some(earliest.map_or(from, |at| at.min(from))))
        })
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
            .max(self.woke_at.unwrap_or(i64::MIN))
    }

    /// One look. While she should not live here nothing is counted and an
    /// alarm that stood is let go: it is not one any more.
    pub fn look(&mut self, seen: Seen) -> Option<Said> {
        self.gaps.retain(|(_, to)| seen.now - *to < GAPS_KEPT_SECS);
        self.host_stops
            .retain(|(_, to)| seen.now - *to < GAPS_KEPT_SECS);
        if let Some(last) = self.looked_at.replace(seen.now)
            && seen.now - last >= SLEPT_SECS
        {
            // The host stopped from `last` to now. A stretch of hers that
            // was open ends where the host stopped; the hour she has to be
            // well for does not run while the host does not.
            self.host_stops.push((last, seen.now));
            self.woke_at = Some(seen.now);
            if let Some(from) = self.gap_from.take() {
                self.gaps.push((from, last));
            }
            if self.well_from.is_some() {
                self.well_slept += seen.now - last;
            }
        }
        if !seen.should_live {
            *self = Self {
                gaps: std::mem::take(&mut self.gaps),
                looked_at: self.looked_at,
                host_stops: std::mem::take(&mut self.host_stops),
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
            self.well_slept = 0;
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
                if seen.now - since - self.well_slept >= WELL_SECS {
                    self.alarmed_from = None;
                    self.well_from = None;
                    self.well_slept = 0;
                    Some(Said::Back { from, since })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// When the watch could first have seen her drivers run, as of a look
    /// that started at `started_at`: this process started, she came to live
    /// here, the host woke.
    pub fn watching_from(&self, started_at: i64) -> i64 {
        started_at
            .max(self.should_since.unwrap_or(i64::MIN))
            .max(self.woke_at.unwrap_or(i64::MIN))
    }

    /// The troubles short of her stopping, looked at after [`Self::look`]
    /// at the same `now`: `stalled_from` since when one of her drivers has
    /// been stuck ([`stalled`]), `unanswered_from` since when her model has
    /// not answered her ([`UNANSWERED_IN_A_ROW`]). While nothing of hers
    /// runs, a stuck driver is that alarm's, not one of its own: it is not
    /// looked at.
    pub fn look_troubles(
        &mut self,
        now: i64,
        stalled_from: Option<i64>,
        unanswered_from: Option<i64>,
    ) -> Vec<Trouble> {
        if self.should_since.is_none() {
            return Vec::new();
        }
        let mut said = Vec::new();
        if self.gap_from.is_none() && self.alarmed_from.is_none() {
            said.extend(self.stalled.look(now, stalled_from).map(Trouble::Stalled));
        }
        said.extend(
            self.unanswered
                .look(now, unanswered_from)
                .map(Trouble::Unanswered),
        );
        said
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

    /// How many stretches the host itself stopped for in the last day, and
    /// their seconds together.
    pub fn host_stops_of_the_day(&self) -> (usize, i64) {
        let all = self.host_stops.iter();
        all.fold((0, 0), |(count, seconds), (from, to)| {
            (count + 1, seconds + (to - from).max(0))
        })
    }
}
