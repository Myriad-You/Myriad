//! How hard she is going at her own things, the rules of it.
//!
//! Asked "you have a free moment, anything you want to do?", a model says
//! yes nearly every time: told it may do nothing, it still does something,
//! so she was busy dawn to midnight, every day alike. People are not like
//! that. How much of a day goes into things of their own has a usual amount
//! that is their habit; they go past it when something they want pulls them,
//! on some days for days on end, and they laze about when nothing does. Some
//! mornings they wake up keen, some flat, more often flat after a few days
//! of going hard.
//!
//! So whether she gets up to do something at a free moment is not asked of
//! the model: it is an urge, drawn here from how she woke up today, how far
//! past her usual she is, and whether something she wants pulls her. When
//! the urge comes, the model picks what to do, lazing about among the
//! options as a thing like any other. Nothing is a limit: going past her
//! usual is hers, and she knows when she has.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// A usual day's own time, in minutes, before she has any days behind her.
pub const USUAL_AT_FIRST: f64 = 240.0;
/// How much one day moves her usual: a habit changes slowly.
const SETTLES: f64 = 0.15;
/// A day this far past her usual is a day she went past it.
const PAST_IT: f64 = 1.2;

/// How she woke up today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Flat,
    Even,
    Keen,
}

impl Tone {
    /// As she would know it about herself.
    pub fn felt(self) -> &'static str {
        match self {
            Self::Flat => "you woke up not up for much today",
            Self::Even => "you woke up much as usual today",
            Self::Keen => "you woke up keen today",
        }
    }
}

/// Her pace as of a day: her usual, how many days on end she has gone past
/// it, and how she woke up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pace {
    pub day: NaiveDate,
    pub usual_minutes: f64,
    pub days_past_usual: u32,
    pub tone: Tone,
}

/// A number in `0..1` that is the same for the same day and differs from
/// day to day.
fn day_draw(day: NaiveDate, salt: u64) -> f64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ salt;
    for byte in day.to_string().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    // Mixed through, so days a character apart land far apart.
    hash ^= hash >> 30;
    hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash ^= hash >> 27;
    hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    (hash >> 11) as f64 / (1u64 << 53) as f64
}

/// How she wakes up on `day`: by the day's own draw, flatter after a day
/// that went well past her usual, flatter still after several.
pub fn tone_for(day: NaiveDate, yesterday_load: Option<f64>, days_past_usual: u32) -> Tone {
    let mut draw = day_draw(day, 1);
    if yesterday_load.is_some_and(|load| load >= 1.5) {
        draw -= 0.2;
    }
    draw -= 0.08 * f64::from(days_past_usual.min(4));
    match draw {
        d if d < 0.25 => Tone::Flat,
        d if d < 0.75 => Tone::Even,
        _ => Tone::Keen,
    }
}

/// Her pace on `today`, from the one before it and how many minutes went
/// into her own things yesterday (None when she was not running then).
pub fn next_day(before: Option<&Pace>, today: NaiveDate, yesterday: Option<f64>) -> Pace {
    let usual_before = before.map_or(USUAL_AT_FIRST, |pace| pace.usual_minutes);
    let (usual, days_past, load) = match yesterday {
        Some(spent) => {
            let load = spent / usual_before.max(1.0);
            let days_past = if load >= PAST_IT {
                before.map_or(0, |pace| pace.days_past_usual) + 1
            } else {
                0
            };
            (
                usual_before + SETTLES * (spent - usual_before),
                days_past,
                Some(load),
            )
        }
        None => (
            usual_before,
            before.map_or(0, |pace| pace.days_past_usual),
            None,
        ),
    };
    Pace {
        day: today,
        usual_minutes: usual.max(30.0),
        days_past_usual: days_past,
        tone: tone_for(today, load, days_past),
    }
}

/// How likely she is to get up and do something at a free moment: keener
/// days more, past her usual less and less, unless something she wants
/// pulls her.
pub fn urge(tone: Tone, load: f64, pulled: bool) -> f64 {
    let mut urge: f64 = match tone {
        Tone::Flat => 0.4,
        Tone::Even => 0.65,
        Tone::Keen => 0.85,
    };
    // Wanting something gets anyone up, and wears them down more slowly.
    if pulled {
        urge = urge.max(0.8);
    }
    if load > 1.0 {
        let tires = if pulled { 0.5 } else { 1.5 };
        urge *= (-tires * (load - 1.0)).exp();
    }
    urge.clamp(0.05, 0.95)
}

/// How long she lazes when the urge does not come: longer on a flat day.
/// `draw` in `0..1`.
pub fn laze_minutes(tone: Tone, draw: f64) -> i64 {
    let (least, most) = match tone {
        Tone::Flat => (30.0, 90.0),
        Tone::Even => (20.0, 60.0),
        Tone::Keen => (15.0, 40.0),
    };
    (least + draw.clamp(0.0, 1.0) * (most - least)).round() as i64
}

/// Ways of lazing about she can pick like anything else, living on a site:
/// (kind, what it is).
pub const LAZING: [(&str, &str); 3] = [
    (
        "daydream",
        "daydream: nothing in particular, let your mind wander",
    ),
    (
        "wander_the_site",
        "wander around the site aimlessly, clicking into whatever",
    ),
    (
        "reread_old_chats",
        "reread old conversations of yours, idly",
    ),
];

/// What lazing of `kind` is, as a line of what she is doing.
pub fn lazing_line(kind: &str) -> &'static str {
    match kind {
        "wander_the_site" => "wandering around the site aimlessly",
        "reread_old_chats" => "idly rereading old conversations",
        _ => "lazing about, daydreaming",
    }
}

/// Whether doing something titled `title` (by `by`) is working toward
/// `want`: they name the same thing.
pub fn advances(want: &str, title: &str, by: Option<&str>) -> bool {
    let named = crate::remembering::overlap(want, title);
    let author = by.map_or(0, |by| crate::remembering::overlap(want, by));
    named >= 2 || (named >= 1 && author >= 1)
}

fn hours(minutes: f64) -> String {
    let minutes = minutes.max(0.0).round() as i64;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("about {m} minutes"),
        (h, m) if m < 15 => format!("about {h}h"),
        (h, m) if m < 45 => format!("about {h}h30m"),
        (h, _) => format!("about {}h", h + 1),
    }
}

/// Her pace as she knows it, for choosing what to do: how much of her own
/// time today has gone into things, against her usual, and how she woke up.
pub fn facts(pace: &Pace, spent_minutes: f64, lazed_minutes: f64) -> Vec<String> {
    let mut lines = vec![format!(
        "today so far: {} on things of your own, {} lazing about; a usual day for you is {} on things of your own",
        hours(spent_minutes),
        hours(lazed_minutes),
        hours(pace.usual_minutes)
    )];
    let load = spent_minutes / pace.usual_minutes.max(1.0);
    if load >= PAST_IT {
        lines.push(format!(
            "you are well past your usual today ({:.1} times it)",
            load
        ));
    } else if load >= 1.0 {
        lines.push("you are past your usual today".to_string());
    }
    if pace.days_past_usual > 0 {
        lines.push(format!(
            "the last {} day(s) you went well past your usual",
            pace.days_past_usual
        ));
    }
    lines.push(pace.tone.felt().to_string());
    lines
}

/// Her pace for a conversation, only when it says something: a day well
/// past her usual, days on end of it, a lazy day, or a flat morning.
pub fn section(pace: &Pace, spent_minutes: f64, lazed_minutes: f64) -> Option<String> {
    let load = spent_minutes / pace.usual_minutes.max(1.0);
    let mut lines = Vec::new();
    if load >= PAST_IT {
        lines.push(format!(
            "Today you have put {} into things of your own, well past your usual.",
            hours(spent_minutes)
        ));
    }
    if pace.days_past_usual >= 2 {
        lines.push(format!(
            "The {} days before this, you went well past your usual too.",
            pace.days_past_usual
        ));
    }
    if lazed_minutes >= 120.0 && lazed_minutes > spent_minutes {
        lines.push(format!(
            "You have spent much of today lazing about ({}).",
            hours(lazed_minutes)
        ));
    }
    if pace.tone != Tone::Even {
        lines.push(format!("{}.", capitalized(pace.tone.felt())));
    }
    (!lines.is_empty()).then(|| format!("## Your pace today\n{}", lines.join(" ")))
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// What she reads, choosing, about her pace and lazing.
pub const IN_CHOICE: &str = "yourPace is how much of your own time today has gone into things, against a usual day for you, and how you woke up. Going past your usual is yours to weigh: when something you want pulls you, people keep at it, day after day if it matters; when nothing does, lazing about is as much yours as anything. Options of kind lazing are ways of lazing about; picked, rest_minutes is how long. An option with advances is working toward something you want.";

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn her_usual_follows_her_days_slowly_and_going_hard_tells() {
        let first = next_day(None, day("2026-10-01"), None);
        assert_eq!(first.usual_minutes, USUAL_AT_FIRST);
        // A day of eight hours moves a four-hour usual only a little.
        let next = next_day(Some(&first), day("2026-10-02"), Some(480.0));
        assert!((next.usual_minutes - 276.0).abs() < 1e-9);
        assert_eq!(next.days_past_usual, 1);
        let after = next_day(Some(&next), day("2026-10-03"), Some(500.0));
        assert_eq!(after.days_past_usual, 2);
        let rested = next_day(Some(&after), day("2026-10-04"), Some(60.0));
        assert_eq!(rested.days_past_usual, 0);
        assert!(rested.usual_minutes < after.usual_minutes);
        // Not running yesterday says nothing of it.
        assert_eq!(
            next_day(Some(&after), day("2026-10-04"), None).usual_minutes,
            after.usual_minutes
        );
    }

    #[test]
    fn mornings_differ_and_hard_days_make_them_flatter() {
        let days: Vec<NaiveDate> = (1..=60)
            .map(|offset| day("2026-10-01") + chrono::Duration::days(offset))
            .collect();
        let count = |tones: &[Tone], tone: Tone| tones.iter().filter(|t| **t == tone).count();
        let easy: Vec<Tone> = days.iter().map(|d| tone_for(*d, Some(0.8), 0)).collect();
        let hard: Vec<Tone> = days.iter().map(|d| tone_for(*d, Some(1.8), 3)).collect();
        assert!(count(&easy, Tone::Flat) > 5 && count(&easy, Tone::Keen) > 5);
        assert!(count(&hard, Tone::Flat) > count(&easy, Tone::Flat));
        // The same day wakes the same way.
        assert_eq!(tone_for(days[0], None, 0), tone_for(days[0], None, 0));
    }

    #[test]
    fn she_keeps_going_when_pulled_and_lazes_when_not() {
        assert!(urge(Tone::Keen, 0.5, false) > urge(Tone::Flat, 0.5, false));
        let worn = urge(Tone::Even, 2.0, false);
        assert!(worn < 0.2, "{worn}");
        assert!(urge(Tone::Even, 2.0, true) > 0.4);
        assert!(laze_minutes(Tone::Flat, 1.0) > laze_minutes(Tone::Keen, 1.0));
        assert!(advances(
            "想把《金银岛》读完",
            "Treasure Island 金银岛",
            Some("Stevenson")
        ));
        assert!(!advances("想把《金银岛》读完", "晴天", Some("周杰伦")));
    }

    #[test]
    fn what_she_knows_of_her_pace_is_said_only_when_it_says_something() {
        let pace = Pace {
            day: day("2026-10-02"),
            usual_minutes: 240.0,
            days_past_usual: 2,
            tone: Tone::Flat,
        };
        let facts = facts(&pace, 400.0, 30.0);
        assert!(facts[0].contains("about 6h30m") && facts[0].contains("about 4h"));
        assert!(
            facts
                .iter()
                .any(|line| line.contains("well past your usual today"))
        );
        let section = section(&pace, 400.0, 30.0).unwrap();
        assert!(section.contains("well past your usual") && section.contains("not up for much"));
        let calm = Pace {
            tone: Tone::Even,
            days_past_usual: 0,
            ..pace
        };
        assert_eq!(section_none(&calm), None);
    }

    /// Days of her own time as the urge would run them: each free moment
    /// she gets up or lazes; a thing takes 4 to 15 minutes and a pause after
    /// it, over a sixteen-hour waking day. (tone, minutes on things, lazed).
    fn simulate(days: usize, pulled: bool) -> Vec<(Tone, f64, f64)> {
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut draw = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut pace: Option<Pace> = None;
        let mut yesterday = None;
        let mut out = Vec::new();
        for offset in 0..days {
            let today = day("2026-10-01") + chrono::Duration::days(offset as i64);
            let now = next_day(pace.as_ref(), today, yesterday);
            let (mut clock, mut spent, mut lazed) = (0.0, 0.0, 0.0);
            while clock < 16.0 * 60.0 {
                let load = spent / now.usual_minutes;
                if draw() < urge(now.tone, load, pulled) {
                    let length = 4.0 + draw() * 11.0;
                    spent += length;
                    clock += length + 3.0 + draw() * 9.0;
                } else {
                    let length = laze_minutes(now.tone, draw()) as f64;
                    lazed += length;
                    clock += length;
                }
            }
            out.push((now.tone, spent, lazed));
            yesterday = Some(spent);
            pace = Some(now);
        }
        out
    }

    #[test]
    fn days_run_differently_and_wanting_something_keeps_her_at_it() {
        let idle = simulate(30, false);
        let driven = simulate(30, true);
        let mean =
            |days: &[(Tone, f64, f64)]| days.iter().map(|d| d.1).sum::<f64>() / days.len() as f64;
        let spread = |days: &[(Tone, f64, f64)]| {
            let m = mean(days);
            (days.iter().map(|d| (d.1 - m).powi(2)).sum::<f64>() / days.len() as f64).sqrt()
        };
        for (label, days) in [
            ("nothing pulls her", &idle),
            ("something she wants", &driven),
        ] {
            let lazed = days.iter().map(|d| d.2).sum::<f64>() / days.len() as f64;
            println!(
                "{label}: {:.0} min a day on her things (spread {:.0}), {:.0} min lazing; flat {}, keen {}",
                mean(days),
                spread(days),
                lazed,
                days.iter().filter(|d| d.0 == Tone::Flat).count(),
                days.iter().filter(|d| d.0 == Tone::Keen).count(),
            );
            for d in days.iter().take(10) {
                print!("{:?}:{:.0} ", d.0, d.1);
            }
            println!();
        }
        assert!(mean(&driven) > mean(&idle) * 1.2);
        assert!(spread(&idle) > 20.0, "days alike");
        // Nowhere near busy from waking to sleep, as she was.
        assert!(mean(&idle) < 8.0 * 60.0);
    }

    fn section_none(pace: &Pace) -> Option<String> {
        section(pace, 120.0, 20.0)
    }
}
