//! The dispatch table: each call is a crate function, its input and output
//! as JSON. A call reads only its input; field names are camelCase.
//!
//! Times are microseconds since the Unix epoch (`i64`). Her zone is an IANA
//! name (`Asia/Shanghai`) or a fixed offset (`+08:00`): a name, because the
//! day's edges move with daylight saving and the same name is what her
//! persona carries to every platform.

use std::str::FromStr;

use chrono::{DateTime, FixedOffset, Utc};
use myriad_merope::{affect, making_sense, speaking, timing, vitals};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::Failure;

type Call = fn(&[u8]) -> Result<Value, Failure>;

/// Sorted by name.
const CALLS: &[(&str, Call)] = &[
    ("affect.mood_band", mood_band),
    ("affect.music_limits", music_limits),
    ("affect.music_listening", music_listening),
    ("affect.transition", transition),
    ("making_sense.told_section", told_section),
    ("meta.version", version),
    ("speaking.acquaintance_section", acquaintance_section),
    ("speaking.activity_section", activity_section),
    ("speaking.addressee_section", addressee_section),
    ("speaking.already_told", already_told),
    ("speaking.already_told_section", already_told_section),
    ("speaking.brought_to_mind_section", brought_to_mind_section),
    ("speaking.contract", contract),
    ("speaking.curious_section", curious_section),
    ("speaking.emotion_section", emotion_section),
    ("speaking.group_section", group_section),
    ("speaking.guest_section", guest_section),
    ("speaking.habits_section", habits_section),
    ("speaking.mood_section", mood_section),
    ("speaking.now_section", now_section),
    ("speaking.on_your_mind_section", on_your_mind_section),
    ("speaking.openers_section", openers_section),
    ("speaking.persona", persona),
    ("speaking.recent_section", recent_section),
    ("speaking.remembered_section", remembered_section),
    ("speaking.since_section", since_section),
    ("timing.asleep_for", asleep_for),
    ("timing.past_bedtime", past_bedtime),
    ("vitals.ends_asking", ends_asking),
    ("vitals.leaned_on", leaned_on),
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

fn mood_band(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        mood: f64,
        arousal: f64,
    }
    let req: In = input(raw)?;
    Ok(json!({ "band": affect::mood_band(req.mood, req.arousal) }))
}

#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Affect {
    mood: f64,
    arousal: f64,
    emotion: f64,
    emotion_arousal: f64,
}

impl From<Affect> for affect::Affect {
    fn from(value: Affect) -> Self {
        Self {
            mood: value.mood,
            arousal: value.arousal,
            emotion: value.emotion,
            emotion_arousal: value.emotion_arousal,
        }
    }
}

impl From<affect::Affect> for Affect {
    fn from(value: affect::Affect) -> Self {
        Self {
            mood: value.mood,
            arousal: value.arousal,
            emotion: value.emotion,
            emotion_arousal: value.emotion_arousal,
        }
    }
}

fn music_listening(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        affect: Affect,
        seconds: u32,
    }
    let req: In = input(raw)?;
    let mut after: affect::Affect = req.affect.into();
    affect::apply_music_listening(&mut after, req.seconds);
    Ok(json!({ "affect": Affect::from(after) }))
}

fn music_limits(raw: &[u8]) -> Result<Value, Failure> {
    let Nothing {} = input(raw)?;
    Ok(json!({
        "minSeconds": affect::MUSIC_LISTENING_MIN_SECS,
        "maxSeconds": affect::MUSIC_LISTENING_MAX_SECS,
        "moodCeiling": affect::MUSIC_MOOD_CEILING,
    }))
}

fn transition(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        before: Affect,
        after: Affect,
        cause: String,
        revision: i64,
    }
    let req: In = input(raw)?;
    let transition = affect::MoodTransition::from_affect(
        &req.before.into(),
        &req.after.into(),
        &req.cause,
        req.revision,
    );
    serde_json::to_value(transition).map_err(|error| Failure::BadInput(error.to_string()))
}

fn told_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// (what, how long ago), latest first.
        told: Vec<(String, String)>,
        /// The private title; the group's when absent.
        #[serde(default)]
        title: Option<String>,
    }
    let req: In = input(raw)?;
    Ok(text(match req.title {
        Some(title) => making_sense::told_section_titled(&req.told, &title),
        None => making_sense::told_section(&req.told),
    }))
}

fn version(raw: &[u8]) -> Result<Value, Failure> {
    let Nothing {} = input(raw)?;
    Ok(json!({
        "abi": crate::ABI_VERSION,
        "upstreamCommit": crate::UPSTREAM_COMMIT,
        "calls": names(),
    }))
}

fn acquaintance_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        first_at: Option<i64>,
        days: u32,
        now_at: i64,
    }
    let req: In = input(raw)?;
    let first = req.first_at.map(at).transpose()?;
    Ok(text(Some(speaking::format_acquaintance_section(
        first,
        req.days,
        at(req.now_at)?,
    ))))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Contents {
    contents: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Label {
    label: String,
}

fn activity_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        activity: String,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_activity_section(&req.activity)))
}

fn addressee_section(raw: &[u8]) -> Result<Value, Failure> {
    let req: Label = input(raw)?;
    Ok(text(Some(speaking::addressee_speaking_section(&req.label))))
}

fn already_told(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        about: String,
        hers: Vec<String>,
    }
    let req: In = input(raw)?;
    let hers: Vec<&str> = req.hers.iter().map(String::as_str).collect();
    Ok(json!({ "told": speaking::already_told(&req.about, &hers) }))
}

fn already_told_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        told: Vec<String>,
    }
    let req: In = input(raw)?;
    let told: Vec<&str> = req.told.iter().map(String::as_str).collect();
    Ok(text(speaking::format_already_told_section(&told)))
}

fn brought_to_mind_section(raw: &[u8]) -> Result<Value, Failure> {
    let req: Contents = input(raw)?;
    Ok(text(speaking::format_brought_to_mind_section(
        &req.contents,
    )))
}

fn contract(raw: &[u8]) -> Result<Value, Failure> {
    let Nothing {} = input(raw)?;
    Ok(text(Some(speaking::PERSONA_SPEAKING_CONTRACT.to_string())))
}

fn curious_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        gap: String,
        known: usize,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_curious_section(&req.gap, req.known)))
}

fn emotion_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        emotion: f64,
        emotion_arousal: f64,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_emotion_section(
        req.emotion,
        req.emotion_arousal,
    )))
}

fn group_section(raw: &[u8]) -> Result<Value, Failure> {
    let req: Label = input(raw)?;
    Ok(text(Some(speaking::group_speaking_section(&req.label))))
}

fn guest_section(raw: &[u8]) -> Result<Value, Failure> {
    let Nothing {} = input(raw)?;
    Ok(text(Some(speaking::guest_speaking_section())))
}

fn habits_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// Her replies in this talk, oldest first.
        hers: Vec<String>,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_habits_section(&req.hers)))
}

fn mood_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        mood: f64,
        arousal: f64,
    }
    let req: In = input(raw)?;
    Ok(text(Some(speaking::format_mood_section(
        req.mood,
        req.arousal,
    ))))
}

fn now_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        at: i64,
        zone: String,
    }
    let req: In = input(raw)?;
    let now = at(req.at)?;
    Ok(text(Some(match Zone::parse(&req.zone)? {
        Zone::Named(zone) => speaking::format_now_section(now.with_timezone(&zone)),
        Zone::Fixed(zone) => speaking::format_now_section(now.with_timezone(&zone)),
    })))
}

fn on_your_mind_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        inner: String,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_on_your_mind_section(&req.inner)))
}

fn openers_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// (how long ago, her first line), oldest first.
        openers: Vec<(String, String)>,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_openers_section(&req.openers)))
}

fn persona(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        name: String,
        personality: String,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_persona(&req.name, &req.personality)))
}

fn recent_section(raw: &[u8]) -> Result<Value, Failure> {
    let req: Contents = input(raw)?;
    Ok(text(speaking::format_recent_section(&req.contents)))
}

fn remembered_section(raw: &[u8]) -> Result<Value, Failure> {
    let req: Contents = input(raw)?;
    Ok(text(speaking::format_remembered_section(&req.contents)))
}

fn since_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        minutes: i64,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_since_section(req.minutes)))
}

fn asleep_for(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        night: u64,
        minute: u32,
    }
    let req: In = input(raw)?;
    Ok(json!({ "seconds": timing::asleep_for(req.night, req.minute) }))
}

fn past_bedtime(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        /// When she would get up, `HH:MM`.
        gets_up: String,
    }
    let req: In = input(raw)?;
    Ok(text(Some(timing::past_bedtime(&req.gets_up))))
}

fn ends_asking(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        reply: String,
    }
    let req: In = input(raw)?;
    Ok(json!({ "asking": vitals::ends_asking(&req.reply) }))
}

fn leaned_on(raw: &[u8]) -> Result<Value, Failure> {
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
