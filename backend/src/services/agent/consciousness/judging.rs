//! How often she judges what one source tells her: at most `PER_HOUR`
//! times in an hour. Past that, something is wrong with the source (a
//! device telling her the same thing without end, a producer gone round
//! in a loop): she stops judging what it tells her for `PAUSED_FOR`, and
//! the site's owner is told so once, with which source it was.
//!
//! A source is the device a child's moment came from, or for the site's
//! own events their kind. A touch is not counted: it comes only from them
//! touching her, at most once a minute (`ingest::produce`).
//!
//! Kept in this process only: a restart starts every count again.

use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use super::ConsciousnessEvent;

const PER_HOUR: usize = 30;
const HOUR: Duration = Duration::from_secs(60 * 60);
const PAUSED_FOR: Duration = HOUR;

/// Whether she judges one more from a source now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Judging {
    Goes,
    /// Past `PER_HOUR` just now: paused from here, and the owner is told.
    PausedNow,
    Paused,
}

#[derive(Default)]
struct Source {
    judged: VecDeque<Instant>,
    paused_until: Option<Instant>,
}

/// (addressee, source) → how it has gone.
#[derive(Default)]
pub(super) struct Sources(HashMap<(i32, String), Source>);

static SOURCES: LazyLock<Mutex<Sources>> = LazyLock::new(|| Mutex::new(Sources::default()));

/// Where an event comes from, as counted here.
pub(super) fn source_of(event: &ConsciousnessEvent) -> Option<String> {
    if event.kind == "agent.merope.touch" {
        return None;
    }
    Some(
        if event.source == "merope" || event.source.trim().is_empty() {
            event.kind.clone()
        } else {
            event.source.clone()
        },
    )
}

impl Sources {
    pub(super) fn judge(&mut self, user_id: i32, source: &str, now: Instant) -> Judging {
        let entry = self.0.entry((user_id, source.to_string())).or_default();
        if let Some(until) = entry.paused_until {
            if now < until {
                return Judging::Paused;
            }
            entry.paused_until = None;
            entry.judged.clear();
        }
        while entry
            .judged
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= HOUR)
        {
            entry.judged.pop_front();
        }
        if entry.judged.len() >= PER_HOUR {
            entry.paused_until = Some(now + PAUSED_FOR);
            return Judging::PausedNow;
        }
        entry.judged.push_back(now);
        Judging::Goes
    }
}

/// Whether she judges `event` now. A source just paused is said in the log
/// and to the site's owner.
pub(super) async fn may_judge(event: &ConsciousnessEvent) -> bool {
    let Some(source) = source_of(event) else {
        return true;
    };
    let judging = SOURCES
        .lock()
        .map(|mut sources| sources.judge(event.addressee_user_id, &source, Instant::now()))
        .unwrap_or(Judging::Goes);
    match judging {
        Judging::Goes => true,
        Judging::Paused => false,
        Judging::PausedNow => {
            tracing::warn!(
                user_id = event.addressee_user_id,
                source,
                kind = %event.kind,
                per_hour = PER_HOUR,
                "[Consciousness] a source asked her to judge too often; paused for an hour"
            );
            if let Some(manager) = crate::services::agent::notifications::get_notification_manager()
            {
                manager
                    .notify_judging_paused(&source, PER_HOUR, PAUSED_FOR.as_secs() / 60)
                    .await;
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_past_its_hour_is_paused_and_said_so_once() {
        let mut sources = Sources::default();
        let start = Instant::now();
        for n in 0..PER_HOUR {
            let at = start + Duration::from_secs(n as u64);
            assert_eq!(sources.judge(1, "mac", at), Judging::Goes, "{n}");
        }
        let past = start + Duration::from_secs(PER_HOUR as u64);
        assert_eq!(sources.judge(1, "mac", past), Judging::PausedNow);
        assert_eq!(sources.judge(1, "mac", past), Judging::Paused);
        // Other sources, and the same source for someone else, go on.
        assert_eq!(
            sources.judge(1, "agent.merope.thought", past),
            Judging::Goes
        );
        assert_eq!(sources.judge(2, "mac", past), Judging::Goes);
        // Paused for its hour even though the old ones have aged out.
        let in_the_hour = past + HOUR - Duration::from_secs(1);
        assert_eq!(sources.judge(1, "mac", in_the_hour), Judging::Paused);
        assert_eq!(sources.judge(1, "mac", past + PAUSED_FOR), Judging::Goes);
    }

    /// Spread over the hour, as many as it may: never paused.
    #[test]
    fn a_source_within_its_hour_goes_on() {
        let mut sources = Sources::default();
        let start = Instant::now();
        let every = HOUR / PER_HOUR as u32;
        for n in 0..(PER_HOUR * 3) {
            assert_eq!(
                sources.judge(1, "mac", start + every * n as u32),
                Judging::Goes,
                "{n}"
            );
        }
    }

    #[test]
    fn a_touch_is_not_counted_and_the_sites_own_go_by_kind() {
        let mut event = ConsciousnessEvent {
            id: "evt".into(),
            source: "merope".into(),
            kind: "agent.merope.touch".into(),
            headline: "h".into(),
            summary: "s".into(),
            addressee_user_id: 1,
            urgency: super::super::EventUrgency::Normal,
            occurred_at: crate::clock::now(),
            parent_event_id: None,
            safe_facts: Default::default(),
            keeps_memory: false,
        };
        assert_eq!(source_of(&event), None);
        event.kind = "agent.merope.thought".into();
        assert_eq!(source_of(&event).as_deref(), Some("agent.merope.thought"));
        event.source = "device-1".into();
        event.kind = "agent.merope.peer_agent".into();
        assert_eq!(source_of(&event).as_deref(), Some("device-1"));
    }
}
