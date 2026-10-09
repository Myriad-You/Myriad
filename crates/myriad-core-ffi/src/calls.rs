//! The dispatch table: each call is a crate function, its input and output
//! as JSON. A call reads only its input; field names are camelCase.
//!
//! Times are microseconds since the Unix epoch (`i64`). Her zone is an IANA
//! name (`Asia/Shanghai`) or a fixed offset (`+08:00`): a name, because the
//! day's edges move with daylight saving and the same name is what her
//! persona carries to every platform.
//!
//! The calls of one area live in its module below; this file holds the
//! table and what they share.

use std::str::FromStr;

use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::Failure;

mod affect;
mod outfit;
mod own;
mod reach;
mod serial;
mod soup;
mod speaking;
mod style;
mod timing;
mod vitals;

type Call = fn(&[u8]) -> Result<Value, Failure>;

/// Sorted by name.
const CALLS: &[(&str, Call)] = &[
    ("affect.apply_appraisal", affect::apply_appraisal),
    ("affect.lite_appraisal", affect::lite_appraisal),
    ("affect.mood_band", affect::mood_band),
    ("affect.mood_cue", affect::mood_cue),
    ("affect.music_limits", affect::music_limits),
    ("affect.music_listening", affect::music_listening),
    ("affect.persona_baseline", affect::persona_baseline),
    ("affect.pull_push", affect::pull_push),
    ("affect.settle", affect::settle),
    ("affect.transition", affect::transition),
    ("affect.user_utterance", affect::user_utterance),
    ("contrast.describe", style::describe),
    ("contrast.overused", style::overused),
    ("contrast.pieces", style::pieces),
    ("library.csv_records", serial::csv_records),
    ("library.from_aozora", serial::from_aozora),
    ("library.from_gutenberg", serial::from_gutenberg),
    ("making.how_it_went", soup::how_it_went),
    ("making.kept_line", soup::kept_line),
    ("making.make_input", soup::make_input),
    ("making.make_system", soup::make_system),
    ("making.named_in", soup::named_in),
    ("making.offer_own", soup::offer_own),
    ("making.parse_idea", soup::parse_idea),
    ("making.record_line", soup::record_line),
    ("making.untried_at", soup::untried_at),
    ("making.writing_about_own", soup::writing_about_own),
    ("making_sense.told_section", speaking::told_section),
    ("meta.version", version),
    ("outfit.after_reply", outfit::after_reply),
    ("outfit.hold_wear", outfit::hold_wear),
    ("outfit.looks_from_profile", outfit::looks_from_profile),
    ("outfit.resolve", outfit::resolve),
    ("outfit.resolve_directive", outfit::resolve_directive),
    ("outfit.split_wear", outfit::split_wear),
    ("outfit.wardrobe_section", outfit::wardrobe_section),
    ("prompt.untrusted_block", speaking::untrusted_block),
    ("reach.as_text", reach::as_text),
    ("reach.judge_system", reach::judge_system),
    ("reach.parse_judged", reach::parse_judged),
    ("reach.reason", reach::reason),
    ("reach.route", reach::route),
    ("reach.texts", reach::texts),
    ("reach.writing_first", reach::writing_first),
    ("reach.wrote_before", reach::wrote_before),
    ("serial.advance", serial::advance),
    ("serial.clean_aozora", serial::clean_aozora),
    ("serial.clean_gutenberg", serial::clean_gutenberg),
    ("serial.looking_back", serial::looking_back),
    ("serial.out", serial::out),
    ("serial.parts", serial::parts),
    ("serial.texts", serial::texts),
    ("serial.view", serial::view),
    ("sore.carried_section", own::sore_carried_section),
    ("sore.input", own::sore_input),
    ("sore.mood_weighs", own::sore_mood_weighs),
    ("sore.section", own::sore_section),
    ("soup.apply", soup::apply),
    ("soup.judge_input", soup::judge_input),
    ("soup.judge_schema", soup::judge_schema),
    ("soup.section", soup::section),
    ("soup.split_start", soup::split_start),
    ("soup.start_system", soup::start_system),
    ("soup.texts", soup::texts),
    (
        "speaking.acquaintance_section",
        speaking::acquaintance_section,
    ),
    ("speaking.activity_section", speaking::activity_section),
    ("speaking.addressee_section", speaking::addressee_section),
    ("speaking.already_told", speaking::already_told),
    (
        "speaking.already_told_section",
        speaking::already_told_section,
    ),
    ("speaking.bits_section", own::bits_section),
    (
        "speaking.brought_to_mind_section",
        speaking::brought_to_mind_section,
    ),
    ("speaking.contract", speaking::contract),
    ("speaking.curious_section", speaking::curious_section),
    ("speaking.doing_section", own::doing_section),
    ("speaking.emotion_section", speaking::emotion_section),
    ("speaking.group_days_section", own::group_days_section),
    ("speaking.group_section", speaking::group_section),
    ("speaking.guest_section", speaking::guest_section),
    ("speaking.habits_section", speaking::habits_section),
    ("speaking.inner_moment_section", own::inner_moment_section),
    ("speaking.lands_section", own::lands_section),
    ("speaking.mood_section", speaking::mood_section),
    ("speaking.now_section", speaking::now_section),
    (
        "speaking.on_your_mind_section",
        speaking::on_your_mind_section,
    ),
    ("speaking.openers_section", speaking::openers_section),
    ("speaking.own_days_section", own::own_days_section),
    ("speaking.persona", speaking::persona),
    ("speaking.recent_section", speaking::recent_section),
    ("speaking.remembered_section", speaking::remembered_section),
    ("speaking.self_story_section", own::self_story_section),
    ("speaking.since_section", speaking::since_section),
    ("speaking.taste_section", own::taste_section),
    ("speaking.us_section", own::us_section),
    ("speaking.views_section", own::views_section),
    ("style.noticed", style::noticed),
    ("style.unlike", style::unlike),
    ("threads.section", own::threads_section),
    ("timing.asleep_for", timing::asleep_for),
    ("timing.day_at", timing::day_at),
    ("timing.past_bedtime", timing::past_bedtime),
    ("timing.sleep", timing::sleep),
    ("vitals.alerts", vitals::alerts),
    ("vitals.ends_asking", vitals::ends_asking),
    ("vitals.leaned_on", vitals::leaned_on),
];

pub(crate) fn names() -> Vec<&'static str> {
    CALLS.iter().map(|(name, _)| *name).collect()
}

pub(crate) fn dispatch(name: &str, input: &[u8]) -> Result<Value, Failure> {
    let at = CALLS
        .binary_search_by(|(known, _)| (*known).cmp(name))
        .map_err(|_| Failure::UnknownCall(name.to_string()))?;
    (CALLS[at].1)(input)
}

fn input<T: DeserializeOwned>(raw: &[u8]) -> Result<T, Failure> {
    serde_json::from_slice(raw).map_err(|error| Failure::BadInput(error.to_string()))
}

fn text(text: Option<String>) -> Value {
    json!({ "text": text })
}

/// A call that takes nothing still takes an object.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nothing {}

fn at(micros: i64) -> Result<DateTime<Utc>, Failure> {
    DateTime::from_timestamp_micros(micros)
        .ok_or_else(|| Failure::BadInput(format!("time {micros} is out of range")))
}

fn opt_at(micros: Option<i64>) -> Result<Option<DateTime<Utc>>, Failure> {
    micros.map(at).transpose()
}

/// Her zone: an IANA name, or a fixed offset `±HH:MM`.
#[derive(Clone, Copy)]
enum Zone {
    Named(chrono_tz::Tz),
    Fixed(FixedOffset),
}

impl Zone {
    fn parse(raw: &str) -> Result<Self, Failure> {
        if let Ok(zone) = chrono_tz::Tz::from_str(raw) {
            return Ok(Self::Named(zone));
        }
        FixedOffset::from_str(raw)
            .map(Self::Fixed)
            .map_err(|_| Failure::BadInput(format!("unknown zone {raw:?}")))
    }
}

/// `$body` with `$zone` bound to the zone itself, whichever kind it is: the
/// crate's rules are generic over `chrono::TimeZone`.
macro_rules! on_zone {
    ($zone:expr, |$bound:ident| $body:expr) => {
        match $zone {
            $crate::calls::Zone::Named($bound) => $body,
            $crate::calls::Zone::Fixed($bound) => $body,
        }
    };
}
use on_zone;

fn version(raw: &[u8]) -> Result<Value, Failure> {
    let Nothing {} = input(raw)?;
    Ok(json!({
        "abi": crate::ABI_VERSION,
        "upstreamCommit": crate::UPSTREAM_COMMIT,
        "calls": names(),
    }))
}
