//! When she sees a message in a chat app, and how long typing takes her
//! (see `myriad_merope::timing`). Facing someone on the site waits for none
//! of this.

use std::time::Duration;

use chrono::{Datelike, Timelike};
use myriad_merope::timing::{self, Where};

/// Seconds until she wakes, if she is asleep now: one night for all of her,
/// in chat apps and in her own time alike.
pub fn asleep_now() -> Option<f64> {
    let local = super::clock::local_now();
    // A night is the date she went to bed: before noon, yesterday's.
    let date = if local.hour() < 12 {
        local.date_naive() - chrono::Duration::days(1)
    } else {
        local.date_naive()
    };
    let night = u64::try_from(date.num_days_from_ce()).unwrap_or_default();
    timing::asleep_for(night, local.hour() * 60 + local.minute())
}

/// Where she is in her own day now (see `myriad_merope::timing::day_at`).
pub fn her_day() -> timing::DayAt {
    let local = super::clock::local_now();
    timing::day_at(
        u64::try_from(local.date_naive().num_days_from_ce()).unwrap_or_default(),
        local.hour() * 60 + local.minute(),
    )
}

/// Her day's date: the day she last got up on, so it turns when she wakes,
/// not at midnight while she is still up.
pub fn her_date() -> chrono::NaiveDate {
    let since_up = chrono::Duration::minutes(i64::from(her_day().since_up));
    (super::clock::local_now() - since_up).date_naive()
}

/// When she would be asleep by her own hours now: told to her, with when
/// she gets up.
pub fn past_bedtime() -> Option<String> {
    let wakes_in = asleep_now()?;
    let up = super::clock::local_now() + chrono::Duration::seconds(wakes_in as i64);
    Some(timing::past_bedtime(&up.format("%H:%M").to_string()))
}

/// What she is doing as a message comes: talking there just now (which
/// keeps her up past her bedtime, as talk does), asleep (where
/// `may_sleep`), in the middle of something of her own, or free.
pub fn where_she_is(in_talk: bool, may_sleep: bool) -> Where {
    if in_talk {
        return Where::InTalk;
    }
    if may_sleep && let Some(wakes_in) = asleep_now() {
        return Where::Asleep { wakes_in };
    }
    let free_in = super::doing::current()
        .map(|doing| (doing.ends - chrono::Utc::now()).num_seconds())
        .filter(|seconds| *seconds > 0);
    match free_in {
        Some(seconds) => Where::Busy {
            free_in: seconds as f64,
        },
        None => Where::Free,
    }
}

/// How long until she has seen `text` and read it.
pub fn until_read(at: Where, text: &str) -> Duration {
    let seconds = timing::seen_after(at, rand::random(), rand::random())
        + timing::reading(text.chars().count());
    Duration::from_secs_f64(seconds)
}

/// How long typing `message` takes her, give or take.
pub fn typing(message: &str) -> Duration {
    Duration::from_secs_f64(timing::typing(message.chars().count(), rand::random()))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    fn at(day: u32, hour: u32, minute: u32) -> chrono::DateTime<chrono::Utc> {
        chrono::Local
            .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .single()
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    #[tokio::test]
    async fn talk_keeps_her_up_and_her_day_turns_when_she_gets_up() {
        use myriad_merope::timing::Where;
        let (night, afternoon, late) = (at(2, 4, 0), at(2, 15, 0), at(2, 0, 30));
        super::super::clock::as_of(night, async {
            assert!(matches!(
                super::where_she_is(false, true),
                Where::Asleep { .. }
            ));
            // Still talking: as much a group's as a private chat's.
            assert!(matches!(super::where_she_is(true, true), Where::InTalk));
            assert!(!matches!(
                super::where_she_is(false, false),
                Where::Asleep { .. }
            ));
            assert!(super::past_bedtime().is_some_and(|told| told.contains("asleep now")));
        })
        .await;
        super::super::clock::as_of(afternoon, async {
            assert!(super::asleep_now().is_none());
            assert!(super::past_bedtime().is_none());
            assert_eq!(super::her_date().to_string(), "2026-10-02");
        })
        .await;
        // Half past midnight she is still up, and still on the day before.
        super::super::clock::as_of(late, async {
            assert!(super::asleep_now().is_none());
            assert_eq!(super::her_date().to_string(), "2026-10-01");
        })
        .await;
    }
}
