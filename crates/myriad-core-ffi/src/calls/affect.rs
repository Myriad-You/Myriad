//! Her mood: the circumplex band, transitions, and how what happens moves
//! it (`myriad_merope::affect`).

use myriad_merope::affect;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Nothing, input};
use crate::Failure;

pub(super) fn mood_band(raw: &[u8]) -> Result<Value, Failure> {
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
pub(super) struct Affect {
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

pub(super) fn music_listening(raw: &[u8]) -> Result<Value, Failure> {
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

pub(super) fn music_limits(raw: &[u8]) -> Result<Value, Failure> {
    let Nothing {} = input(raw)?;
    Ok(json!({
        "minSeconds": affect::MUSIC_LISTENING_MIN_SECS,
        "maxSeconds": affect::MUSIC_LISTENING_MAX_SECS,
        "moodCeiling": affect::MUSIC_MOOD_CEILING,
    }))
}

pub(super) fn transition(raw: &[u8]) -> Result<Value, Failure> {
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
/// The mood her settled affect drifts back to.
#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
pub(super) struct Baseline {
    mood: f64,
    arousal: f64,
}

impl From<Baseline> for affect::AffectBaseline {
    fn from(value: Baseline) -> Self {
        Self {
            mood: value.mood,
            arousal: value.arousal,
        }
    }
}

/// What an appraisal pulls the short emotion toward.
#[derive(Deserialize, Serialize, Clone, Copy)]
#[serde(deny_unknown_fields)]
pub(super) struct Appraisal {
    emotion: f64,
    arousal: f64,
}

fn moved(after: affect::Affect) -> Value {
    json!({ "affect": Affect::from(after) })
}

pub(super) fn pull_push(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        affect: Affect,
    }
    let req: In = input(raw)?;
    let mut after: affect::Affect = req.affect.into();
    affect::pull_push(&mut after);
    Ok(moved(after))
}

pub(super) fn apply_appraisal(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        affect: Affect,
        appraisal: Appraisal,
        scale: f64,
    }
    let req: In = input(raw)?;
    let mut after: affect::Affect = req.affect.into();
    let appraisal = affect::Appraisal {
        emotion: req.appraisal.emotion,
        arousal: req.appraisal.arousal,
    };
    affect::apply_appraisal(&mut after, appraisal, req.scale);
    Ok(moved(after))
}

pub(super) fn lite_appraisal(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        valence: i32,
        arousal: i32,
    }
    let req: In = input(raw)?;
    let appraisal = affect::lite_appraisal(req.valence, req.arousal);
    Ok(json!({ "emotion": appraisal.emotion, "arousal": appraisal.arousal }))
}

pub(super) fn user_utterance(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        affect: Affect,
        /// Which of their messages in this talk it is, from 0.
        index: u32,
        praised: bool,
        scolded: bool,
    }
    let req: In = input(raw)?;
    let mut after: affect::Affect = req.affect.into();
    affect::apply_user_utterance(&mut after, req.index, req.praised, req.scolded);
    Ok(moved(after))
}

pub(super) fn settle(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        affect: Affect,
        base: Baseline,
        mood_hours: f64,
        emotion_hours: f64,
    }
    let req: In = input(raw)?;
    Ok(moved(affect::settle(
        req.affect.into(),
        req.base.into(),
        req.mood_hours,
        req.emotion_hours,
    )))
}

pub(super) fn mood_cue(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        text: String,
    }
    let req: In = input(raw)?;
    let (praised, scolded) = affect::detect_mood_cue(&req.text);
    Ok(json!({ "praised": praised, "scolded": scolded }))
}

pub(super) fn persona_baseline(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// Her persona as stored (`temperament`, `socialStyle`, …); null when
        /// there is none.
        persona: Option<Value>,
        personality: String,
    }
    let req: In = input(raw)?;
    let base = affect::persona_affect_baseline(req.persona.as_ref(), &req.personality);
    Ok(json!({ "mood": base.mood, "arousal": base.arousal }))
}
