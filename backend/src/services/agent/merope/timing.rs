//! When she sees a message in a chat app, and how long typing takes her
//! (see `myriad_merope::timing`). Facing someone on the site waits for none
//! of this.

use std::time::Duration;

use chrono::{Datelike, Timelike};
use myriad_merope::timing::{self, Where};

/// What she is doing as a message comes: asleep (where `may_sleep`),
/// talking there just now, in the middle of something of her own, or free.
pub fn where_she_is(in_talk: bool, may_sleep: bool) -> Where {
    let local = chrono::Local::now();
    if may_sleep {
        // A night is the date she went to bed: before noon, yesterday's.
        let date = if local.hour() < 12 {
            local.date_naive() - chrono::Duration::days(1)
        } else {
            local.date_naive()
        };
        let night = u64::try_from(date.num_days_from_ce()).unwrap_or_default();
        if let Some(wakes_in) = timing::asleep_for(night, local.hour() * 60 + local.minute()) {
            return Where::Asleep { wakes_in };
        }
    }
    if in_talk {
        return Where::InTalk;
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
