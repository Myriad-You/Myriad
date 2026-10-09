//! Her vital signs: habits counted in her replies, and what in a day is
//! worth raising (`myriad_merope::vitals`).

use myriad_merope::vitals;
use serde::Deserialize;
use serde_json::{Value, json};

use super::input;
use crate::Failure;

pub(super) fn ends_asking(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        reply: String,
    }
    let req: In = input(raw)?;
    Ok(json!({ "asking": vitals::ends_asking(&req.reply) }))
}

pub(super) fn leaned_on(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        texts: Vec<String>,
        share: f64,
        most: usize,
    }
    let req: In = input(raw)?;
    Ok(json!({ "leaned": vitals::leaned_on(&req.texts, req.share, req.most) }))
}

pub(super) fn alerts(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// A day as `vitals::Day` serializes it.
        today: vitals::Day,
        /// The days before it, oldest first.
        before: Vec<vitals::Day>,
    }
    let req: In = input(raw)?;
    Ok(json!({ "alerts": vitals::alerts(&req.today, &req.before) }))
}
