//! What goes into her prompt: the sections of a speaking prompt
//! (`myriad_merope::speaking`, `making_sense`), and the fence around
//! outside text (`myriad_agent_rules::untrusted_block`).

use myriad_merope::{making_sense, speaking};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Nothing, Zone, at, input, on_zone, text};
use crate::Failure;

pub(super) fn told_section(raw: &[u8]) -> Result<Value, Failure> {
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

pub(super) fn acquaintance_section(raw: &[u8]) -> Result<Value, Failure> {
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
pub(super) struct Contents {
    contents: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Label {
    label: String,
}

pub(super) fn activity_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        activity: String,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_activity_section(&req.activity)))
}

pub(super) fn addressee_section(raw: &[u8]) -> Result<Value, Failure> {
    let req: Label = input(raw)?;
    Ok(text(Some(speaking::addressee_speaking_section(&req.label))))
}

pub(super) fn already_told(raw: &[u8]) -> Result<Value, Failure> {
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

pub(super) fn already_told_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        told: Vec<String>,
    }
    let req: In = input(raw)?;
    let told: Vec<&str> = req.told.iter().map(String::as_str).collect();
    Ok(text(speaking::format_already_told_section(&told)))
}

pub(super) fn brought_to_mind_section(raw: &[u8]) -> Result<Value, Failure> {
    let req: Contents = input(raw)?;
    Ok(text(speaking::format_brought_to_mind_section(
        &req.contents,
    )))
}

pub(super) fn contract(raw: &[u8]) -> Result<Value, Failure> {
    let Nothing {} = input(raw)?;
    Ok(text(Some(speaking::PERSONA_SPEAKING_CONTRACT.to_string())))
}

pub(super) fn curious_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        gap: String,
        known: usize,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_curious_section(&req.gap, req.known)))
}

pub(super) fn emotion_section(raw: &[u8]) -> Result<Value, Failure> {
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

pub(super) fn group_section(raw: &[u8]) -> Result<Value, Failure> {
    let req: Label = input(raw)?;
    Ok(text(Some(speaking::group_speaking_section(&req.label))))
}

pub(super) fn guest_section(raw: &[u8]) -> Result<Value, Failure> {
    let Nothing {} = input(raw)?;
    Ok(text(Some(speaking::guest_speaking_section())))
}

pub(super) fn habits_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// Her replies in this talk, oldest first.
        hers: Vec<String>,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_habits_section(&req.hers)))
}

pub(super) fn mood_section(raw: &[u8]) -> Result<Value, Failure> {
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

pub(super) fn now_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        at: i64,
        zone: String,
    }
    let req: In = input(raw)?;
    let now = at(req.at)?;
    let zone = Zone::parse(&req.zone)?;
    Ok(text(Some(on_zone!(zone, |zone| {
        speaking::format_now_section(now.with_timezone(&zone))
    }))))
}

pub(super) fn on_your_mind_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        inner: String,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_on_your_mind_section(&req.inner)))
}

pub(super) fn openers_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// (how long ago, her first line), oldest first.
        openers: Vec<(String, String)>,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_openers_section(&req.openers)))
}

pub(super) fn persona(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        name: String,
        personality: String,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_persona(&req.name, &req.personality)))
}

pub(super) fn recent_section(raw: &[u8]) -> Result<Value, Failure> {
    let req: Contents = input(raw)?;
    Ok(text(speaking::format_recent_section(&req.contents)))
}

pub(super) fn remembered_section(raw: &[u8]) -> Result<Value, Failure> {
    let req: Contents = input(raw)?;
    Ok(text(speaking::format_remembered_section(&req.contents)))
}

pub(super) fn since_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        minutes: i64,
    }
    let req: In = input(raw)?;
    Ok(text(speaking::format_since_section(req.minutes)))
}

pub(super) fn untrusted_block(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        tag: String,
        body: String,
    }
    let req: In = input(raw)?;
    Ok(text(Some(myriad_agent_rules::untrusted_block(
        &req.tag, &req.body,
    ))))
}
