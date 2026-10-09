//! Her hours: when she sleeps, and where she is in her own day
//! (`myriad_merope::timing`).

use myriad_merope::timing;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{input, text};
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
