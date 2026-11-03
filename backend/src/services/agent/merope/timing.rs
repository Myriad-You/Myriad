//! Her day by the site's clock (see `myriad_merope::timing`): when she
//! sleeps, where she is in her day, and her draws. When she sees a message
//! and how long typing takes her are `chat_typing`'s.

use myriad_merope::timing;

/// Seconds until she wakes, if she is asleep now: one night for all of her,
/// in chat apps and in her own time alike (see
/// `myriad_merope::timing::asleep_at`).
pub fn asleep_now() -> Option<f64> {
    #[cfg(test)]
    if let Some(asleep) = crate::services::channel_group::g0::asleep_for() {
        return asleep;
    }
    timing::asleep_at(super::clock::now(), &super::clock::zone())
}

/// Where she is in her own day now (see `myriad_merope::timing::day_at`).
pub fn her_day() -> timing::DayAt {
    timing::her_day_at(super::clock::now(), &super::clock::zone())
}

/// Her day's date: the day she last got up on, so it turns when she wakes,
/// not at midnight while she is still up.
pub fn her_date() -> chrono::NaiveDate {
    timing::her_date_at(super::clock::now(), &super::clock::zone())
}

/// When she would be asleep by her own hours now: told to her, with when
/// she gets up.
pub fn past_bedtime() -> Option<String> {
    timing::past_bedtime_at(super::clock::now(), &super::clock::zone())
}

/// A number uniform in [0, 1): a draw of hers.
pub(crate) fn roll() -> f64 {
    #[cfg(test)]
    if let Some(roll) = crate::services::channel_group::g0::roll() {
        return roll;
    }
    rand::random()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    fn at(day: u32, hour: u32, minute: u32) -> chrono::DateTime<chrono::Utc> {
        super::super::clock::zone()
            .with_ymd_and_hms(2026, 10, day, hour, minute, 0)
            .single()
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    #[tokio::test]
    async fn talk_keeps_her_up_and_her_day_turns_when_she_gets_up() {
        use myriad_merope::timing::{Where, where_she_is};
        let (night, afternoon, late) = (at(2, 4, 0), at(2, 15, 0), at(2, 0, 30));
        super::super::clock::as_of(night, async {
            let asleep = super::asleep_now();
            assert!(matches!(
                where_she_is(false, true, asleep, None),
                Where::Asleep { .. }
            ));
            // Still talking: as much a group's as a private chat's.
            assert!(matches!(
                where_she_is(true, true, asleep, None),
                Where::InTalk
            ));
            assert!(!matches!(
                where_she_is(false, false, asleep, None),
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
