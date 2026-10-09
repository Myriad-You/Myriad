//! Writing to someone first (`myriad_merope::reach`).

use myriad_merope::reach;
use serde::Deserialize;
use serde_json::{Value, json};

use super::own::Thread;
use super::{Nothing, at, input, opt_at, text};
use crate::Failure;

pub(super) fn texts(raw: &[u8]) -> Result<Value, Failure> {
    let Nothing {} = input(raw)?;
    Ok(json!({
        "notMidTalkSeconds": reach::NOT_MID_TALK.num_seconds(),
        "thinksBackDays": reach::THINKS_BACK_DAYS,
        "judgeSchemaName": reach::JUDGE_SCHEMA,
        "judgeSchema": reach::judge_schema(),
        "maxLineChars": reach::MAX_LINE_CHARS,
    }))
}

pub(super) fn route(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        panel_open: bool,
        on_site: bool,
    }
    let req: In = input(raw)?;
    let route = match reach::route(req.panel_open, req.on_site) {
        reach::Route::Stay => "Stay",
        reach::Route::Site => "Site",
        reach::Route::Away => "Away",
    };
    Ok(json!({ "route": route }))
}

pub(super) fn reason(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        threads: Vec<Thread>,
        #[serde(default)]
        last: Option<i64>,
        to_tell: Vec<String>,
        #[serde(default)]
        to_try: Option<String>,
        now: i64,
    }
    let req: In = input(raw)?;
    let threads = req
        .threads
        .into_iter()
        .map(Thread::into_crate)
        .collect::<Result<Vec<_>, _>>()?;
    let reason = reach::reason(
        &threads,
        opt_at(req.last)?,
        req.to_tell,
        req.to_try,
        at(req.now)?,
    );
    let ids = |threads: &[myriad_merope::threads::Thread]| -> Vec<String> {
        threads.iter().map(|thread| thread.id.clone()).collect()
    };
    Ok(json!({
        "reason": reason.map(|reason| json!({
            "key": reason.key(),
            "due": ids(&reason.due),
            "wished": ids(&reason.wished),
            "daysSince": reason.days_since,
            "toTell": reason.to_tell,
            "toTry": reason.to_try,
        })),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Soul {
    soul: String,
}

pub(super) fn judge_system(raw: &[u8]) -> Result<Value, Failure> {
    let req: Soul = input(raw)?;
    Ok(text(Some(reach::judge_system(&req.soul))))
}

pub(super) fn parse_judged(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        raw: String,
    }
    let req: In = input(raw)?;
    Ok(match reach::parse_judged(&req.raw) {
        None => json!({ "readable": false, "about": null }),
        Some(about) => json!({ "readable": true, "about": about }),
    })
}

pub(super) fn wrote_before(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// (how long ago, what), newest first.
        lines: Vec<(String, String)>,
    }
    let req: In = input(raw)?;
    Ok(text(reach::wrote_before(&req.lines)))
}

pub(super) fn writing_first(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        about: String,
        #[serde(default)]
        last_talked: Option<String>,
    }
    let req: In = input(raw)?;
    Ok(text(Some(reach::writing_first(
        &req.about,
        req.last_talked.as_deref(),
    ))))
}

pub(super) fn as_text(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        raw: String,
    }
    let req: In = input(raw)?;
    Ok(text(reach::as_text(&req.raw)))
}
