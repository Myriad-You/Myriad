//! Her hours: when she sleeps, and where she is in her own day
//! (`myriad_merope::timing`).

use myriad_merope::timing;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Zone, at, input, on_zone, text};
use crate::Failure;

pub(super) fn asleep_for(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        night: u64,
        minute: u32,
    }
    let req: In = input(raw)?;
    Ok(json!({ "seconds": timing::asleep_for(req.night, req.minute) }))
}

pub(super) fn past_bedtime(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        /// When she would get up, `HH:MM`.
        gets_up: String,
    }
    let req: In = input(raw)?;
    Ok(text(Some(timing::past_bedtime(&req.gets_up))))
}

pub(super) fn sleep(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        night: u64,
    }
    let req: In = input(raw)?;
    let (bed, up) = timing::sleep(req.night);
    Ok(json!({ "bed": bed, "up": up }))
}

fn day(at: timing::DayAt) -> Value {
    json!({ "asleep": at.asleep, "sinceUp": at.since_up, "untilBed": at.until_bed })
}

pub(super) fn day_at(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        day: u64,
        minute: u32,
    }
    let req: In = input(raw)?;
    Ok(day(timing::day_at(req.day, req.minute)))
}

/// A moment, and her zone it is read on.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Now {
    now: i64,
    zone: String,
}

impl Now {
    fn read(raw: &[u8]) -> Result<(chrono::DateTime<chrono::Utc>, Zone), Failure> {
        let req: Self = input(raw)?;
        Ok((at(req.now)?, Zone::parse(&req.zone)?))
    }
}

pub(super) fn asleep_at(raw: &[u8]) -> Result<Value, Failure> {
    let (now, zone) = Now::read(raw)?;
    Ok(json!({ "seconds": on_zone!(zone, |zone| timing::asleep_at(now, &zone)) }))
}

pub(super) fn her_day_at(raw: &[u8]) -> Result<Value, Failure> {
    let (now, zone) = Now::read(raw)?;
    Ok(day(on_zone!(zone, |zone| timing::her_day_at(now, &zone))))
}

pub(super) fn her_date_at(raw: &[u8]) -> Result<Value, Failure> {
    let (now, zone) = Now::read(raw)?;
    let date = on_zone!(zone, |zone| timing::her_date_at(now, &zone));
    Ok(json!({ "date": date.format("%Y-%m-%d").to_string() }))
}

pub(super) fn past_bedtime_at(raw: &[u8]) -> Result<Value, Failure> {
    let (now, zone) = Now::read(raw)?;
    Ok(text(on_zone!(zone, |zone| timing::past_bedtime_at(
        now, &zone
    ))))
}
