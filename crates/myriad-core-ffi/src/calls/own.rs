//! Her own life as it goes into her prompt: her own time, her views and
//! story, what someone is to her, the days behind her, what she has on her
//! mind about someone (`threads`) and what still stings (`sore`).

use myriad_merope::{sore, speaking, threads};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Zone, at, input, on_zone, opt_at, text};
use crate::Failure;

/// `(a, b)` pairs, as `[a, b]` arrays.
type Pairs = Vec<(String, String)>;

pub(super) fn doing_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// What she is in the middle of, as one line.
        #[serde(default)]
        now: Option<String>,
        /// (what, what stayed with her), latest first.
        lately: Pairs,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_doing_section(
        req.now.as_deref(),
        &req.lately,
    )))
}

pub(super) fn views_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// (about, view).
        views: Pairs,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_views_section(&req.views)))
}

pub(super) fn taste_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        liked_by: Vec<String>,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_taste_section(&req.liked_by)))
}

pub(super) fn self_story_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        claims: Vec<String>,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_self_story_section(&req.claims)))
}

pub(super) fn us_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct First {
        text: String,
        at: i64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// How she puts it now, and when she put it so.
        now: String,
        since: i64,
        /// How she had put it just before, if she had.
        #[serde(default)]
        before: Option<String>,
        /// How she first put it, and when.
        #[serde(default)]
        first: Option<First>,
        today: i64,
    }
    let req: In = input(raw)?;
    let first = match &req.first {
        Some(first) => Some((first.text.as_str(), at(first.at)?)),
        None => None,
    };
    Ok(text(speaking::format_us_section(
        &req.now,
        at(req.since)?,
        req.before.as_deref(),
        first,
        at(req.today)?,
    )))
}

pub(super) fn inner_moment_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        inner: String,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_inner_moment_ago_section(&req.inner)))
}

pub(super) fn lands_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        lands: String,
        since: i64,
        today: i64,
        group: bool,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_lands_section(
        &req.lands,
        at(req.since)?,
        at(req.today)?,
        req.group,
    )))
}

pub(super) fn group_days_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// (date, what it was like), oldest first.
        days: Pairs,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_group_days_section(&req.days)))
}

pub(super) fn bits_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// (handle, how it goes).
        bits: Pairs,
        group: bool,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_bits_section(&req.bits, req.group)))
}

pub(super) fn own_days_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        days: Vec<String>,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_own_days_section(&req.days)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Thread {
    #[serde(default)]
    id: String,
    about: String,
    then: String,
    #[serde(default)]
    due: Option<i64>,
    hers: bool,
}

impl Thread {
    pub(super) fn into_crate(self) -> Result<threads::Thread, Failure> {
        Ok(threads::Thread {
            id: self.id,
            about: self.about,
            then: self.then,
            due: opt_at(self.due)?,
            hers: self.hers,
        })
    }
}

pub(super) fn threads_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        threads: Vec<Thread>,
        now: i64,
        zone: String,
        /// For a group's talk; with them otherwise.
        group: bool,
    }
    let req: In = input(raw)?;
    let threads = req
        .threads
        .into_iter()
        .map(Thread::into_crate)
        .collect::<Result<Vec<_>, _>>()?;
    let now = at(req.now)?;
    let zone = Zone::parse(&req.zone)?;
    Ok(text(on_zone!(zone, |zone| if req.group {
        threads::group_section(&threads, now, &zone)
    } else {
        threads::section(&threads, now, &zone)
    })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sore {
    what: String,
    /// `petty`, `hurt` or `deep`.
    weight: String,
    since: i64,
    #[serde(default)]
    mended: Option<i64>,
    /// `private`, or the group's venue (`group:…`).
    venue: String,
    #[serde(default)]
    who: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sores {
    sores: Vec<Sore>,
    now: i64,
}

impl Sores {
    fn read(raw: &[u8]) -> Result<(Vec<sore::Sore>, chrono::DateTime<chrono::Utc>), Failure> {
        let req: Self = input(raw)?;
        let sores = req
            .sores
            .into_iter()
            .map(|kept| {
                Ok(sore::Sore {
                    id: String::new(),
                    user_id: 0,
                    weight: sore::Weight::parse(&kept.weight).ok_or_else(|| {
                        Failure::BadInput(format!("unknown weight {:?}", kept.weight))
                    })?,
                    what: kept.what,
                    since: at(kept.since)?,
                    mended: opt_at(kept.mended)?,
                    venue: kept.venue,
                    who: kept.who,
                })
            })
            .collect::<Result<Vec<_>, Failure>>()?;
        Ok((sores, at(req.now)?))
    }
}

pub(super) fn sore_input(raw: &[u8]) -> Result<Value, Failure> {
    let (sores, now) = Sores::read(raw)?;
    Ok(json!({ "input": sore::as_input(&sores, now) }))
}

pub(super) fn sore_section(raw: &[u8]) -> Result<Value, Failure> {
    let (sores, now) = Sores::read(raw)?;
    Ok(text(sore::section(&sores, now)))
}

pub(super) fn sore_carried_section(raw: &[u8]) -> Result<Value, Failure> {
    let (sores, now) = Sores::read(raw)?;
    Ok(text(sore::carried_section(&sores, now)))
}

pub(super) fn sore_mood_weighs(raw: &[u8]) -> Result<Value, Failure> {
    let (sores, _) = Sores::read(raw)?;
    Ok(json!({ "weighs": sore::mood_weighs(&sores) }))
}
