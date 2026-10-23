//! What "now" is while her words are put together: the clock, except where
//! a turn is put together as of another moment (a simulated day, a real
//! turn answered again), so that the hour, how long ago things were, and
//! what has come due are as they were then. Scoped to the one task putting
//! the prompt together; nothing else sees it.
//!
//! And the zone her day runs in: the one her persona names (`zone`), or the
//! site's own when it names none.

use std::{fmt, str::FromStr, sync::RwLock};

use chrono::{
    DateTime, FixedOffset, Local, MappedLocalTime, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc,
};

tokio::task_local! {
    static AT: DateTime<Utc>;
}

#[cfg(test)]
tokio::task_local! {
    static IN_ZONE: Zone;
}

/// Now, as this turn is put together.
pub fn now() -> DateTime<Utc> {
    #[cfg(test)]
    if let Some(now) = crate::services::channel_group::g0::now() {
        return AT.try_with(|at| *at).unwrap_or(now);
    }
    AT.try_with(|at| *at)
        .unwrap_or_else(|_| crate::clock::now())
}

/// The zone her day runs in: the one her persona names, or the site's own,
/// which is the process's (`TZ`). The rules in `myriad_merope` take it as an
/// input and never read the process zone themselves; nor does anything else
/// of hers here: every day, night and month of hers is in this zone.
pub fn zone() -> Zone {
    #[cfg(test)]
    if let Ok(zone) = IN_ZONE.try_with(|zone| *zone) {
        return zone;
    }
    // A recorded scene's day runs where its world is (+08:00).
    #[cfg(test)]
    if crate::services::channel_group::g0::on() {
        return Zone::Named(chrono_tz::Asia::Shanghai);
    }
    ZONE.read().map(|zone| *zone).unwrap_or(Zone::Site)
}

/// [`now`], in her zone.
pub fn local_now() -> DateTime<Zone> {
    now().with_timezone(&zone())
}

/// Today's date in her zone (midnight to midnight; her own day, which turns
/// when she gets up, is `timing::her_date`).
pub fn today() -> NaiveDate {
    local_now().date_naive()
}

/// The date `at` falls on in her zone.
pub fn date_of<Tz: TimeZone>(at: &DateTime<Tz>) -> NaiveDate {
    at.with_timezone(&zone()).date_naive()
}

/// When `day` begins in her zone: its first midnight (the earlier one when
/// the clocks go back), none when the clocks skip it.
pub fn day_start(day: NaiveDate) -> Option<DateTime<FixedOffset>> {
    zone()
        .from_local_datetime(&day.and_hms_opt(0, 0, 0)?)
        .earliest()
        .map(|start| start.fixed_offset())
}

/// The zone her persona row names, as it was last read or written: every
/// read of her persona keeps this current, so a change made through another
/// replica arrives with the next read. Empty, unknown, or no persona at all
/// is the site's own zone.
pub fn note_zone(named: Option<&str>) {
    let zone = named.and_then(Zone::named).unwrap_or(Zone::Site);
    if let Ok(mut held) = ZONE.write() {
        *held = zone;
    }
}

static ZONE: RwLock<Zone> = RwLock::new(Zone::Site);

/// Her zone: an IANA zone, or the site's own (the process's `Local`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Named(chrono_tz::Tz),
    Site,
}

impl Zone {
    /// An IANA name (`Asia/Shanghai`), trimmed; none when empty or unknown.
    pub fn named(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        chrono_tz::Tz::from_str(raw).ok().map(Self::Named)
    }

    /// Whether `raw` is a zone her persona may name: empty (the site's own)
    /// or a known IANA name.
    pub fn is_valid_name(raw: &str) -> bool {
        raw.trim().is_empty() || Self::named(raw).is_some()
    }
}

/// An offset in her zone, carrying the zone it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneOffset {
    zone: Zone,
    fixed: FixedOffset,
}

impl Offset for ZoneOffset {
    fn fix(&self) -> FixedOffset {
        self.fixed
    }
}

impl fmt::Display for ZoneOffset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.fixed, f)
    }
}

impl Zone {
    fn offset(self, fixed: FixedOffset) -> ZoneOffset {
        ZoneOffset { zone: self, fixed }
    }
}

impl TimeZone for Zone {
    type Offset = ZoneOffset;

    fn from_offset(offset: &ZoneOffset) -> Self {
        offset.zone
    }

    fn offset_from_local_date(&self, local: &NaiveDate) -> MappedLocalTime<ZoneOffset> {
        match self {
            Zone::Named(tz) => tz
                .offset_from_local_date(local)
                .map(|offset| self.offset(offset.fix())),
            Zone::Site => Local
                .offset_from_local_date(local)
                .map(|offset| self.offset(offset)),
        }
    }

    fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<ZoneOffset> {
        match self {
            Zone::Named(tz) => tz
                .offset_from_local_datetime(local)
                .map(|offset| self.offset(offset.fix())),
            Zone::Site => Local
                .offset_from_local_datetime(local)
                .map(|offset| self.offset(offset)),
        }
    }

    fn offset_from_utc_date(&self, utc: &NaiveDate) -> ZoneOffset {
        match self {
            Zone::Named(tz) => self.offset(tz.offset_from_utc_date(utc).fix()),
            Zone::Site => self.offset(Local.offset_from_utc_date(utc)),
        }
    }

    fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> ZoneOffset {
        match self {
            Zone::Named(tz) => self.offset(tz.offset_from_utc_datetime(utc).fix()),
            Zone::Site => self.offset(Local.offset_from_utc_datetime(utc)),
        }
    }
}

/// Put `work` together as of `at`.
#[cfg(test)]
pub(crate) async fn as_of<F: std::future::Future>(at: DateTime<Utc>, work: F) -> F::Output {
    AT.scope(at, work).await
}

/// Run `work` with her day in `zone`, whatever her persona names: the zone
/// is otherwise one for the whole process, which tests share.
#[cfg(test)]
pub(crate) async fn in_zone<F: std::future::Future>(zone: Zone, work: F) -> F::Output {
    IN_ZONE.scope(zone, work).await
}

#[cfg(test)]
mod tests {
    use chrono::{Datelike, Timelike};

    use super::*;

    #[tokio::test]
    async fn a_turn_put_together_as_of_then_sees_then_and_nothing_else_does() {
        let then = crate::clock::now() - chrono::Duration::days(3);
        let seen = super::as_of(then, async { super::now() }).await;
        assert_eq!(seen, then);
        assert!(crate::clock::now() - super::now() < chrono::Duration::seconds(5));
    }

    #[test]
    fn her_zone_is_a_named_one_or_the_sites_own() {
        assert_eq!(Zone::named(""), None);
        assert_eq!(Zone::named("  "), None);
        assert_eq!(Zone::named("Mars/Olympus"), None);
        assert!(Zone::is_valid_name(""));
        assert!(Zone::is_valid_name(" Europe/Berlin "));
        assert!(!Zone::is_valid_name("+08:00"));
        let berlin = Zone::named("Europe/Berlin").unwrap();
        // Summer and winter: the named zone's own rules, not a fixed offset.
        let summer = Utc.with_ymd_and_hms(2026, 7, 1, 12, 0, 0).unwrap();
        let winter = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        assert_eq!(summer.with_timezone(&berlin).hour(), 14);
        assert_eq!(winter.with_timezone(&berlin).hour(), 13);
        // Late at night in Berlin is already tomorrow in Shanghai.
        let late = Utc.with_ymd_and_hms(2026, 7, 1, 21, 30, 0).unwrap();
        let shanghai = Zone::named("Asia/Shanghai").unwrap();
        assert_eq!(late.with_timezone(&berlin).day(), 1);
        assert_eq!(late.with_timezone(&shanghai).day(), 2);
        // The site's own is the process zone, to the second.
        let here = late.with_timezone(&Zone::Site);
        assert_eq!(here.naive_local(), late.with_timezone(&Local).naive_local());
        // A local midnight in the zone maps back to the same instant.
        let midnight = NaiveDate::from_ymd_opt(2026, 3, 29)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let start = berlin.from_local_datetime(&midnight).earliest().unwrap();
        assert_eq!(start.naive_local(), midnight);
        assert_eq!(start.timezone(), berlin);
    }

    /// Her today, as each place that counts her days reads it, at one moment.
    async fn her_days_at(at: DateTime<Utc>, zone: Zone) -> serde_json::Value {
        use super::super::{chat_remember, life, pace, stickers, vitals};
        in_zone(
            zone,
            as_of(at, async {
                let today = today();
                let day = |bounds: Option<(DateTime<FixedOffset>, DateTime<FixedOffset>)>| {
                    bounds.map(|(start, end)| [start.to_rfc3339(), end.to_rfc3339()])
                };
                let extract: serde_json::Value =
                    serde_json::from_str(&chat_remember::extract_input(
                        "hi",
                        "hello",
                        &super::super::TurnContext::default(),
                        local_now(),
                    ))
                    .unwrap();
                serde_json::json!({
                    "today": today.to_string(),
                    "dateOfNow": date_of(&now()).to_string(),
                    "starts": day_start(today).map(|start| start.to_rfc3339()),
                    "pace": day(pace::bounds(today)),
                    "vitals": day(vitals::bounds(today, today)),
                    "life": day(life::day_bounds(today)),
                    "night": life::is_night(&local_now()),
                    "month": local_now().format("%Y-%m").to_string(),
                    "stickersFrom": stickers::month_start().map(|start| start.to_rfc3339()),
                    "extractToday": extract["today"],
                })
            }),
        )
        .await
    }

    #[tokio::test]
    async fn her_day_turns_in_her_zone_wherever_it_is_counted() {
        // 13:00 on 31 July in Los Angeles is 05:00 on 1 August in Tokyo:
        // another day, another month, and her night there.
        let at = Utc.with_ymd_and_hms(2026, 7, 31, 20, 0, 0).unwrap();
        let la = her_days_at(at, Zone::named("America/Los_Angeles").unwrap()).await;
        let day = ["2026-07-31T07:00:00+00:00", "2026-08-01T07:00:00+00:00"];
        assert_eq!(
            la,
            serde_json::json!({
                "today": "2026-07-31",
                "dateOfNow": "2026-07-31",
                "starts": "2026-07-31T00:00:00-07:00",
                "pace": ["2026-07-31T00:00:00-07:00", "2026-08-01T00:00:00-07:00"],
                "vitals": ["2026-07-31T00:00:00-07:00", "2026-08-01T00:00:00-07:00"],
                "life": ["2026-07-31T00:00:00-07:00", "2026-08-01T00:00:00-07:00"],
                "night": false,
                "month": "2026-07",
                "stickersFrom": "2026-07-01T07:00:00+00:00",
                "extractToday": "2026-07-31 (Friday)",
            })
        );
        let la_day = la["pace"].as_array().unwrap();
        for (bound, utc) in la_day.iter().zip(day) {
            assert_eq!(
                DateTime::parse_from_rfc3339(bound.as_str().unwrap()).unwrap(),
                DateTime::parse_from_rfc3339(utc).unwrap()
            );
        }
        let tokyo = her_days_at(at, Zone::named("Asia/Tokyo").unwrap()).await;
        assert_eq!(
            tokyo,
            serde_json::json!({
                "today": "2026-08-01",
                "dateOfNow": "2026-08-01",
                "starts": "2026-08-01T00:00:00+09:00",
                "pace": ["2026-08-01T00:00:00+09:00", "2026-08-02T00:00:00+09:00"],
                "vitals": ["2026-08-01T00:00:00+09:00", "2026-08-02T00:00:00+09:00"],
                "life": ["2026-08-01T00:00:00+09:00", "2026-08-02T00:00:00+09:00"],
                "night": true,
                "month": "2026-08",
                "stickersFrom": "2026-07-31T15:00:00+00:00",
                "extractToday": "2026-08-01 (Saturday)",
            })
        );
        // With no zone of her own, the site's: what the process clock says.
        let site = her_days_at(at, Zone::Site).await;
        let here = at.with_timezone(&Local);
        let midnight = |day: NaiveDate| {
            Local
                .from_local_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
                .earliest()
                .unwrap()
                .to_rfc3339()
        };
        let today = here.date_naive();
        assert_eq!(site["today"], today.to_string());
        assert_eq!(site["starts"], midnight(today));
        assert_eq!(
            site["pace"],
            serde_json::json!([midnight(today), midnight(today.succ_opt().unwrap())])
        );
        assert_eq!(site["vitals"], site["pace"]);
        assert_eq!(site["month"], here.format("%Y-%m").to_string());
        assert_eq!(site["night"], (3..=5).contains(&here.hour()));
    }
}
