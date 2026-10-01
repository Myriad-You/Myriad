//! When she sees a message in a chat app, and how long her answer takes to
//! type. A person is not holding the phone: the wait before they see a line
//! is long-tailed and depends on what they are doing (Barabási 2005;
//! Malmgren et al. 2008), fast while a conversation is going (Wu et al.
//! 2010), and nothing at all while they sleep. The words are the model's;
//! the when is here. Facing someone on the site is talking face to face and
//! waits for none of this.
//!
//! Reference: in one active QQ group, members answered someone who @'d them
//! after 27 s at the median (19 s to 54 s between the quartiles); she
//! answered after 3 s (tests/merope/talk-reference.json).

use serde::{Deserialize, Serialize};

/// What she is doing when a message comes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Where {
    /// Talking there just now: the phone is in her hand.
    InTalk,
    /// Nothing of her own on: she looks now and then.
    Free,
    /// In the middle of something of her own, free in so many seconds; she
    /// glances at the phone meanwhile, or sees it when she is done.
    Busy { free_in: f64 },
    /// Asleep, awake in so many seconds.
    Asleep { wakes_in: f64 },
}

/// A wait whose logarithm is normal: most are near the median, a few are
/// much longer. `u1`, `u2` are uniform in [0, 1).
fn long_tailed(median: f64, spread: f64, u1: f64, u2: f64) -> f64 {
    let u1 = u1.clamp(1e-9, 1.0 - 1e-9);
    let normal = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
    median * (spread * normal).exp()
}

/// Seconds until she sees a message. Medians and spreads are fitted so a
/// day's mix lands among the group's members (see the test); they are not
/// measured one by one.
pub fn seen_after(at: Where, u1: f64, u2: f64) -> f64 {
    const IN_TALK: (f64, f64) = (5.0, 0.6);
    const FREE: (f64, f64) = (20.0, 0.8);
    const GLANCE_WHILE_BUSY: (f64, f64) = (60.0, 0.9);
    const AFTER_WAKING: (f64, f64) = (300.0, 0.5);
    match at {
        Where::InTalk => long_tailed(IN_TALK.0, IN_TALK.1, u1, u2),
        Where::Free => long_tailed(FREE.0, FREE.1, u1, u2),
        Where::Busy { free_in } => long_tailed(GLANCE_WHILE_BUSY.0, GLANCE_WHILE_BUSY.1, u1, u2)
            .min(free_in.max(0.0) + 5.0),
        Where::Asleep { wakes_in } => {
            wakes_in.max(0.0) + long_tailed(AFTER_WAKING.0, AFTER_WAKING.1, u1, u2)
        }
    }
}

/// Seconds to read a message of `chars` characters. Silent reading runs
/// about 240 words a minute (Brysbaert 2019); for Chinese characters, a
/// guess of 8 a second.
pub fn reading(chars: usize) -> f64 {
    chars as f64 / 8.0
}

/// Seconds to type a message of `chars` characters on a phone, give or
/// take (`u` uniform in [0, 1)). In the group, a member's next message in a
/// row came 6 s after the last at the median, about 7 characters.
pub fn typing(chars: usize, u: f64) -> f64 {
    const LONGEST: f64 = 15.0;
    ((0.5 + 0.7 * chars as f64) * (0.7 + 0.6 * u)).min(LONGEST)
}

/// When she sleeps, in minutes after local midnight: to bed about 01:30 and
/// up about 08:30, each night up to 45 minutes either way. `night` is any
/// number that stays the same through one night (the date she went to bed).
pub fn sleep(night: u64) -> (u32, u32) {
    // A small mix so neighbouring nights differ; no need for more.
    let mut x = night.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 31;
    let jitter = |x: u64| (x % 91) as i64 - 45;
    let bed = 90 + jitter(x);
    let up = 510 + jitter(x >> 17);
    (bed.rem_euclid(1440) as u32, up as u32)
}

/// Seconds until she wakes, if she is asleep at `minute` after local
/// midnight on a night that started `night` (see `sleep`).
pub fn asleep_for(night: u64, minute: u32) -> Option<f64> {
    let (bed, up) = sleep(night);
    let asleep = if bed < up {
        minute >= bed && minute < up
    } else {
        minute >= bed || minute < up
    };
    asleep.then(|| {
        let left = (up + 1440 - minute) % 1440;
        f64::from(left) * 60.0
    })
}

/// Where she is in her own day at `minute` after local midnight on calendar
/// day `day` (any day count, the same one `sleep` is given its nights in):
/// asleep or not, minutes since she last got up, and until she next goes to
/// bed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayAt {
    pub asleep: bool,
    pub since_up: u32,
    pub until_bed: u32,
}

pub fn day_at(day: u64, minute: u32) -> DayAt {
    let now = day as i64 * 1440 + i64::from(minute);
    let mut at = DayAt {
        asleep: false,
        since_up: 0,
        until_bed: 0,
    };
    let mut bed_found = false;
    // The night before yesterday's, last night and tonight: a night is
    // the date she went to bed, and she goes to bed after midnight.
    for night in [day.saturating_sub(2), day.saturating_sub(1), day] {
        let (bed, up) = sleep(night);
        let bed_at = (night as i64 + 1) * 1440 + i64::from(bed);
        let up_at = (night as i64 + 1) * 1440 + i64::from(up);
        if bed_at <= now && now < up_at {
            at.asleep = true;
        }
        if up_at <= now {
            at.since_up = (now - up_at) as u32;
        }
        if bed_at > now && !bed_found {
            at.until_bed = (bed_at - now) as u32;
            bed_found = true;
        }
    }
    at
}

/// Told to her when she would be asleep by her own hours: she may have
/// been woken, or stayed up talking. When she would get up, `HH:MM`.
pub fn past_bedtime(gets_up: &str) -> String {
    format!(
        "## Your hours\nBy your usual hours you would be asleep now; you get up about {gets_up}."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn her_day_runs_from_getting_up_to_going_to_bed() {
        let day = 739_525;
        let (bed_tonight, _) = sleep(day);
        let (_, up_today) = sleep(day - 1);
        // Mid-afternoon: up since this morning, bed after midnight tonight.
        let afternoon = day_at(day, 15 * 60);
        assert!(!afternoon.asleep);
        assert_eq!(afternoon.since_up, 15 * 60 - up_today);
        assert_eq!(afternoon.until_bed, 9 * 60 + bed_tonight);
        // Just after midnight she is still up, from yesterday morning.
        let (bed_last_night, up_this_morning) = sleep(day - 1);
        let (_, up_yesterday) = sleep(day - 2);
        let late = day_at(day, 10);
        assert!(!late.asleep && bed_last_night > 10);
        assert_eq!(late.since_up, 1440 + 10 - up_yesterday);
        assert_eq!(late.until_bed, bed_last_night - 10);
        // Asleep agrees with `asleep_for` all day long.
        for minute in (0..1440).step_by(7) {
            let night = if minute < 12 * 60 { day - 1 } else { day };
            assert_eq!(
                day_at(day, minute).asleep,
                asleep_for(night, minute).is_some(),
                "minute {minute}"
            );
        }
        assert!(day_at(day, up_this_morning - 1).asleep);
        assert!(past_bedtime("08:20").contains("about 08:20"));
    }

    /// A fixed stream of rolls, so a run is the same every time.
    struct Rolls(u64);

    impl Rolls {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    fn quantile(values: &mut [f64], q: f64) -> f64 {
        values.sort_by(f64::total_cmp);
        values[((values.len() as f64 * q) as usize).min(values.len() - 1)]
    }

    /// Thirty days of people @'ing her in the day: some while she is in the
    /// talk, some while she is free, some while she is busy with her own
    /// things. What she takes to answer lands among what the group's members
    /// take, not at the 3 s she used to answer in.
    #[test]
    fn she_answers_an_at_about_when_a_member_would() {
        let mut rolls = Rolls(7);
        let mut waits = Vec::new();
        for _ in 0..30 * 40 {
            let at = match rolls.next() {
                r if r < 0.4 => Where::InTalk,
                r if r < 0.7 => Where::Free,
                _ => Where::Busy {
                    free_in: rolls.next() * 1800.0,
                },
            };
            let (u1, u2) = (rolls.next(), rolls.next());
            // What she writes back: about as long as a member's message.
            let chars = 4 + (rolls.next() * 12.0) as usize;
            let model = 3.0 + rolls.next() * 3.0;
            let typed = typing(chars, rolls.next()).max(model);
            waits.push(seen_after(at, u1, u2) + reading(10) + typed);
        }
        let median = quantile(&mut waits, 0.5);
        let (low, high) = (quantile(&mut waits, 0.25), quantile(&mut waits, 0.75));
        assert!((19.0..=40.0).contains(&median), "median {median}");
        assert!((8.0..=25.0).contains(&low), "p25 {low}");
        assert!((30.0..=90.0).contains(&high), "p75 {high}");
        // Long-tailed: a few waits are many times the median.
        let p95 = quantile(&mut waits, 0.95);
        assert!(p95 > 3.0 * median, "p95 {p95}");
    }

    #[test]
    fn in_talk_she_is_quick_and_busy_she_waits_at_most_until_she_is_free() {
        let mut rolls = Rolls(11);
        let mut talk = Vec::new();
        let mut busy = Vec::new();
        for _ in 0..2000 {
            let (u1, u2) = (rolls.next(), rolls.next());
            talk.push(seen_after(Where::InTalk, u1, u2));
            busy.push(seen_after(Where::Busy { free_in: 20.0 }, u1, u2));
        }
        assert!(quantile(&mut talk, 0.5) < 6.0);
        assert!(busy.iter().all(|wait| *wait <= 25.0));
    }

    #[test]
    fn she_sleeps_every_night_at_about_the_same_time() {
        let mut beds = Vec::new();
        for night in 0..365u64 {
            let (bed, up) = sleep(night);
            assert!((45..=135).contains(&bed), "bed {bed}");
            assert!((465..=555).contains(&up), "up {up}");
            beds.push(bed);
            // Asleep through the middle of the night, awake at noon.
            assert!(asleep_for(night, 4 * 60).is_some());
            assert!(asleep_for(night, 12 * 60).is_none());
            assert!(asleep_for(night, 23 * 60).is_none());
        }
        // Not the same minute every night.
        beds.sort_unstable();
        beds.dedup();
        assert!(beds.len() > 30);
        // What is left of the night is counted to the minute she gets up.
        let (_, up) = sleep(3);
        assert_eq!(asleep_for(3, up - 1), Some(60.0));
        assert_eq!(asleep_for(3, up), None);
        let asleep_at_four = asleep_for(3, 240).unwrap();
        assert_eq!(asleep_at_four, f64::from(up - 240) * 60.0);
        // After waking, a few minutes more before she looks.
        let wait = seen_after(
            Where::Asleep {
                wakes_in: asleep_at_four,
            },
            0.5,
            0.25,
        );
        assert!(wait > asleep_at_four && wait < asleep_at_four + 900.0);
    }

    #[test]
    fn typing_takes_longer_for_more_and_never_forever() {
        assert!(typing(2, 0.5) < typing(12, 0.5));
        assert!((typing(7, 0.5) - 5.4).abs() < 0.01);
        assert_eq!(typing(500, 0.99), 15.0);
        assert!(reading(16) < reading(80));
    }
}
