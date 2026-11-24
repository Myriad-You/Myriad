//! How far a device has come in sending her log on this link: what it last
//! said it holds of an audience (`log.vector`), and whether the page its
//! last batch ended is followed by another (`log.rows` `more`). Forgets go
//! ahead of everything else (`myriad_nodes::wire`), so what one of them
//! means for other rows is read once the rest of it came.

use std::collections::{BTreeMap, BTreeSet};

use myriad_persona_log::Vector;

use super::NODE;

/// One device's, on one link, by audience.
#[derive(Debug, Clone, Default)]
pub struct Rounds {
    device: String,
    by_audience: BTreeMap<String, Round>,
    /// [`Self::over`]: the ends that may still send any.
    live: Option<BTreeSet<String>>,
    taken_in: bool,
}

#[derive(Debug, Clone, Default)]
struct Round {
    theirs: Vector,
    more: bool,
}

impl Rounds {
    /// No link's: what `live` may still send is not waited for here.
    pub fn over(live: BTreeSet<String>) -> Self {
        Self {
            live: Some(live),
            ..Self::default()
        }
    }

    /// Whether her nodes the log has were taken in on this link and nothing
    /// failed since: until then that is owed, whatever is news or not (a
    /// link before may have failed at it).
    pub fn taken_in(&self) -> bool {
        self.taken_in
    }

    /// They were taken in, or that failed.
    pub fn note_taken_in(&mut self, taken_in: bool) {
        self.taken_in = taken_in;
    }

    /// `device` said it holds `theirs` of `audience`: whether that is news.
    pub fn vector(&mut self, device: &str, audience: &str, theirs: &Vector) -> bool {
        self.device = device.to_string();
        let news = self
            .by_audience
            .get(audience)
            .is_none_or(|round| round.theirs != *theirs);
        self.by_audience
            .entry(audience.to_string())
            .or_default()
            .theirs = theirs.clone();
        news
    }

    /// A batch of `audience` came from `device`, ending a page another
    /// follows (`more`) or not: whether the last of them came with it.
    pub fn batch(&mut self, device: &str, audience: &str, more: bool) -> bool {
        self.device = device.to_string();
        let round = self.by_audience.entry(audience.to_string()).or_default();
        std::mem::replace(&mut round.more, more) && !more
    }

    /// Whether everything `origin` wrote in `audience` up to its count
    /// `dseq` is in her log (`held`, its vector), and so is what this device
    /// has to give of it: the row of that count came by `from`. What the
    /// server wrote itself is. With no link ([`Self::over`]): whether all of
    /// it up to there is, and neither end is one that may still send any. Up
    /// to a gap it is not: the same computer linked anew under another id
    /// sends what it wrote as the one before.
    pub fn through(
        &self,
        audience: &str,
        held: &Vector,
        origin: &str,
        dseq: u64,
        from: &str,
    ) -> bool {
        if origin == NODE {
            return true;
        }
        let held = held.get(origin).copied().unwrap_or(0);
        if held < dseq {
            return false;
        }
        if let Some(live) = &self.live {
            return !live.contains(origin) && !live.contains(from);
        }
        let Some(round) = self.by_audience.get(audience).filter(|round| !round.more) else {
            return false;
        };
        let theirs = round.theirs.get(origin).copied().unwrap_or(0);
        held >= theirs && (from == self.device || theirs >= dseq)
    }
}
