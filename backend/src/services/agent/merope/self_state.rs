//! The persona's own state, not toward anyone: her energy, her want to know,
//! and the facts of her day.
//!
//! Derived, never stored. It is read off facts that already exist — the hour
//! of her day, when each person last talked to her, when she last learned
//! something — so it cannot drift from what actually happened, survives
//! restarts, and needs no reset.
//!
//! - **Energy** follows her own day (low near her bedtime and when woken,
//!   slow just after she gets up) and drops with how many
//!   different people she has been talking to lately. Five people wear her out
//!   more than five turns with one.
//! - **How long since she learned something new** (a fact about someone,
//!   something looked up or found out, something heard): a fact, not a
//!   score. Her everyday doings are not learning.
//!
//! Two different uses, kept apart:
//! - **Mechanical knobs** the system turns from these numbers: how long she
//!   waits between speaking up unprompted, how far recall wanders by
//!   association, how often her mind drifts. They budget cost and intrusion.
//! - **What the model is told** is only the facts underneath ([`SelfFacts`]):
//!   the time, how many people talked with her lately, how long since anyone
//!   came, how long since she learned something new. How tired or curious she
//!   is, what she does about it and how she talks, is the model's judgment,
//!   never an instruction. Scores never reach a prompt.

use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Timelike, Utc};
use sea_orm::DatabaseConnection;

/// Energy each recent person costs at the moment they spoke.
const FATIGUE_PER_PERSON: f64 = 12.0;
/// A conversation's weight on energy falls by e every this many hours.
const FATIGUE_TAU_H: f64 = 1.5;
/// Only this far back counts toward either.
const LOOKBACK_H: i64 = 24;
const CACHE_FOR: Duration = Duration::from_secs(30);
/// The shortest wait between unprompted words; tiredness only lengthens it.
pub const BASE_PROACTIVE_COOLDOWN_SECS: i64 = 180;

/// What happened, as plain facts, for the model to judge from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelfFacts {
    pub local_hour: u32,
    pub local_minute: u32,
    /// Different people who talked with her in the last few hours.
    pub people_recently: usize,
    /// Since anyone last talked to her; `None` when nobody has in a day.
    pub hours_since_anyone: Option<f64>,
    /// Since she last learned anything new about anyone; `None`: never.
    pub hours_since_learned: Option<f64>,
}

/// How far back "lately" reaches for the people count.
const RECENT_PEOPLE_H: f64 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelfState {
    /// 0–100.
    pub energy: f64,
    pub facts: SelfFacts,
}

impl SelfState {
    pub fn tired(&self) -> bool {
        self.energy < 40.0
    }

    /// The same state, knowing how long ago she last learned something new
    /// (`None`: never).
    pub fn with_last_learned(self, hours_ago: Option<f64>) -> Self {
        let hours = hours_ago
            .filter(|hours| hours.is_finite() && *hours >= 0.0)
            .unwrap_or(f64::INFINITY);
        Self {
            facts: SelfFacts {
                hours_since_learned: hours.is_finite().then_some(hours),
                ..self.facts
            },
            ..self
        }
    }

    /// How long to wait after speaking up before speaking up again. Never
    /// shorter than the base; a tired persona leaves people alone longer.
    pub fn proactive_cooldown_secs(&self) -> i64 {
        let spent = 1.0 - self.energy.clamp(0.0, 100.0) / 100.0;
        (BASE_PROACTIVE_COOLDOWN_SECS as f64 * (1.0 + 3.0 * spent * spent)).round() as i64
    }

    /// How far recall may wander by association, `0..=1`. Tiredness narrows
    /// thought to what was actually said.
    pub fn recall_breadth(&self) -> f64 {
        if self.tired() { 0.5 } else { 1.0 }
    }

    /// The facts for a decision model, which judges for itself.
    pub fn facts_view(&self) -> crate::services::agent::consciousness::SelfFacts {
        let facts = self.facts;
        crate::services::agent::consciousness::SelfFacts {
            local_time: format!("{:02}:{:02}", facts.local_hour, facts.local_minute),
            people_recently: facts.people_recently,
            hours_since_anyone: facts.hours_since_anyone.map(round_tenth),
            hours_since_learned: facts.hours_since_learned.map(round_tenth),
        }
    }
}

/// Energy at this point of her own day, before anyone has tired her: by
/// how long until she goes to bed and since she got up, not the clock, so
/// it moves with her hours.
fn day_energy(at: myriad_merope::timing::DayAt) -> f64 {
    if at.asleep {
        return 25.0;
    }
    if at.since_up < 60 {
        return 55.0;
    }
    match at.until_bed {
        0..=90 => 25.0,
        91..=150 => 35.0,
        151..=210 => 50.0,
        211..=450 => 65.0,
        _ => 75.0,
    }
}

/// [`derive_at`] on a day of her usual hours (to bed 01:30, up 08:30).
#[cfg(test)]
pub fn derive(hour: u32, contacts: &[f64]) -> SelfState {
    let minute = hour * 60;
    let at = myriad_merope::timing::DayAt {
        asleep: (90..510).contains(&minute),
        since_up: (minute + 1440 - 510) % 1440,
        until_bed: (90 + 1440 - minute) % 1440,
    };
    derive_at(at, hour, contacts)
}

/// `at` is where she is in her own day, `hour` the clock's; `contacts`
/// are, for each person, hours since they last talked to her.
pub fn derive_at(
    at: myriad_merope::timing::DayAt,
    hour: u32,
    contacts: &[f64],
) -> SelfState {
    let load: f64 = contacts
        .iter()
        .filter(|hours| hours.is_finite() && **hours >= 0.0)
        .map(|hours| (-hours / FATIGUE_TAU_H).exp())
        .sum();
    let energy = (day_energy(at) - FATIGUE_PER_PERSON * load).clamp(5.0, 100.0);
    let valid: Vec<f64> = contacts
        .iter()
        .copied()
        .filter(|hours| hours.is_finite() && *hours >= 0.0)
        .collect();
    SelfState {
        energy,
        facts: SelfFacts {
            local_hour: hour,
            local_minute: 0,
            people_recently: valid
                .iter()
                .filter(|hours| **hours <= RECENT_PEOPLE_H)
                .count(),
            hours_since_anyone: valid.iter().copied().reduce(f64::min),
            hours_since_learned: None,
        },
    }
}

fn round_tenth(hours: f64) -> f64 {
    (hours * 10.0).round() / 10.0
}

static CACHE: LazyLock<Mutex<Option<(Instant, SelfState)>>> = LazyLock::new(|| Mutex::new(None));

/// Her state now. Recomputed at most every half minute.
pub async fn current(db: &DatabaseConnection) -> SelfState {
    if let Some(state) = CACHE
        .lock()
        .ok()
        .and_then(|cached| *cached)
        .filter(|(at, _)| at.elapsed() < CACHE_FOR)
        .map(|(_, state)| state)
    {
        return state;
    }
    let now = Utc::now();
    let contacts = recent_contacts(db, now).await.unwrap_or_default();
    let learned = crate::services::agent::memory::unified::last_learned_at(db)
        .await
        .ok()
        .flatten()
        .map(|at| (now - at.with_timezone(&Utc)).num_seconds().max(0) as f64 / 3600.0);
    // Her day runs on the host clock, as the do-not-disturb window does.
    let local = super::clock::local_now();
    let mut state =
        derive_at(super::timing::her_day(), local.hour(), &contacts).with_last_learned(learned);
    state.facts.local_minute = local.minute();
    if let Ok(mut cached) = CACHE.lock() {
        *cached = Some((Instant::now(), state));
    }
    state
}

async fn recent_contacts(
    db: &DatabaseConnection,
    now: DateTime<Utc>,
) -> Result<Vec<f64>, sea_orm::DbErr> {
    let since = (now - chrono::Duration::hours(LOOKBACK_H)).fixed_offset();
    Ok(super::store::last_talked_since(db, since)
        .await?
        .into_iter()
        .map(|at| (now - at.with_timezone(&Utc)).num_seconds().max(0) as f64 / 3600.0)
        .collect())
}

fn roughly(hours: f64) -> String {
    if hours < 1.0 {
        "less than an hour ago".into()
    } else if hours < 1.5 {
        "about an hour ago".into()
    } else if hours < 24.0 {
        format!("about {} hours ago", hours.round())
    } else {
        "more than a day ago".into()
    }
}

/// Her own day as plain facts. What it does to her — tired, restless,
/// curious, glad of company — is left to the model and the personality.
pub fn format_day_section(facts: &SelfFacts) -> String {
    // The clock is in the Now section, on every turn.
    let mut lines = Vec::new();
    lines.push(match facts.people_recently {
        0 => "Nobody else has talked with you in the last few hours.".to_string(),
        1 => "One person has talked with you in the last few hours.".to_string(),
        n => format!("{n} different people have talked with you in the last few hours."),
    });
    if let Some(hours) = facts.hours_since_anyone.filter(|hours| *hours >= 1.0) {
        lines.push(format!(
            "Before this, the last time anyone came was {}.",
            roughly(hours)
        ));
    }
    lines.push(match facts.hours_since_learned {
        Some(hours) => format!("You last learned something new {}.", roughly(hours)),
        None => "You have not learned anything new yet.".to_string(),
    });
    format!("## Your day\n{}", lines.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_night_and_many_people_tire_her() {
        assert!(derive(14, &[]).energy >= 65.0);
        assert!(derive(3, &[]).tired(), "night");
        // Her own hours, not the clock's: still up at 00:30 an hour before
        // bed is tired; at 09:00, two hours after getting up, she is not.
        use myriad_merope::timing::DayAt;
        let up_late = DayAt {
            asleep: false,
            since_up: 16 * 60,
            until_bed: 60,
        };
        assert!(derive_at(up_late, 0, &[]).tired());
        let morning = DayAt {
            asleep: false,
            since_up: 120,
            until_bed: 16 * 60,
        };
        assert_eq!(derive_at(morning, 9, &[]).energy, 75.0);
        let woken = DayAt {
            asleep: true,
            since_up: 20 * 60,
            until_bed: 22 * 60,
        };
        assert!(derive_at(woken, 9, &[]).tired(), "woken before she gets up");
        let one = derive(14, &[0.1]);
        assert!(!one.tired());
        let crowd = derive(14, &[0.1, 0.2, 0.2, 0.3, 0.5]);
        assert!(crowd.tired(), "{crowd:?}");
        let long_ago = derive(14, &[10.0, 11.0, 12.0, 13.0, 14.0]);
        assert!(long_ago.energy >= 65.0, "rested since: {long_ago:?}");
    }

    #[test]
    fn silence_is_a_fact_she_is_told() {
        assert_eq!(
            derive(14, &[]).facts.hours_since_anyone,
            None,
            "no one all day"
        );
        assert_eq!(derive(14, &[5.0, 7.0]).facts.hours_since_anyone, Some(5.0));
        assert_eq!(derive(14, &[5.0, 7.0]).facts.people_recently, 1);
    }

    #[test]
    fn tiredness_only_lengthens_the_wait_and_narrows_recall() {
        let fresh = SelfState {
            energy: 100.0,
            ..derive(14, &[])
        };
        let spent = SelfState {
            energy: 5.0,
            ..derive(14, &[])
        };
        assert_eq!(
            fresh.proactive_cooldown_secs(),
            BASE_PROACTIVE_COOLDOWN_SECS
        );
        assert!(spent.proactive_cooldown_secs() > 3 * BASE_PROACTIVE_COOLDOWN_SECS);
        assert_eq!(fresh.recall_breadth(), 1.0);
        assert!(spent.recall_breadth() < 1.0);
    }

    #[test]
    fn how_long_since_she_learned_something_is_a_fact_not_a_score() {
        let base = derive(14, &[]);
        assert_eq!(base.with_last_learned(None).facts.hours_since_learned, None);
        assert_eq!(
            base.with_last_learned(Some(12.04))
                .facts_view()
                .hours_since_learned,
            Some(12.0)
        );
    }

    #[test]
    fn the_model_is_told_facts_not_how_to_feel() {
        let night = derive(2, &[0.2, 0.5, 1.0, 3.0, 7.0]).with_last_learned(Some(9.2));
        let day = format_day_section(&night.facts);
        // The clock is in the Now section, on every turn.
        assert!(day.starts_with("## Your day\n"));
        assert!(!day.contains("02:00"));
        assert!(day.contains("4 different people have talked with you"));
        assert!(day.contains("about 9 hours ago"));
        for verdict in ["tired", "energy", "shorter", "lonely", "curious", "glad"] {
            assert!(
                !day.contains(verdict),
                "{verdict} is the model's call: {day}"
            );
        }
        let quiet = format_day_section(&derive(14, &[7.0]).facts);
        assert!(quiet.contains("Nobody else has talked with you"));
        assert!(quiet.contains("the last time anyone came was about 7 hours ago"));
        assert!(quiet.contains("have not learned anything new"));
        let view = night.facts_view();
        assert_eq!(view.local_time, "02:00");
        assert_eq!(view.people_recently, 4);
        assert_eq!(view.hours_since_anyone, Some(0.2));
    }
}
