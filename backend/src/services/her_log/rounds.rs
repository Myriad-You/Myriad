//! How far a device has come in sending her log on this link: what it last
//! said it holds of an audience (`log.vector`), and whether the page its
//! last batch ended is followed by another (`log.rows` `more`). Forgets go
//! ahead of everything else (`myriad_nodes::wire`), so what one of them
//! means for other rows is read once the rest of it came.

use std::collections::BTreeMap;

use myriad_persona_log::Vector;

use super::NODE;

/// One device's, on one link, by audience.
#[derive(Debug, Clone, Default)]
pub struct Rounds {
    device: String,
    by_audience: BTreeMap<String, Round>,
}

#[derive(Debug, Clone, Default)]
struct Round {
    theirs: Vector,
    more: bool,
}

impl Rounds {
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
    /// server wrote itself is.
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
        let Some(round) = self.by_audience.get(audience).filter(|round| !round.more) else {
            return false;
        };
        let held = held.get(origin).copied().unwrap_or(0);
        let theirs = round.theirs.get(origin).copied().unwrap_or(0);
        held >= dseq && held >= theirs && (from == self.device || theirs >= dseq)
    }
}
