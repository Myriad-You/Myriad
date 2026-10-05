//! The director's performance score: a run of beats, each a held pose change
//! or a there-and-back move, placed on a moment. A beat lands on words of the
//! reply as they are spoken, or at a time after the score arrives, so a score
//! plays the same whether she is speaking, listening, thinking or idle.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::performance::{BodyPose, sanitize_body_pose};
use crate::rig_contract::{
    PERFORMANCE_BODY_CONTROLS, PERFORMANCE_SCORE_DIRECTIONS, PERFORMANCE_SCORE_MOVES,
    PERFORMANCE_SCORE_SIDES, SCORE_MAX_ANCHOR_CHARS, SCORE_MAX_AT_MS, SCORE_MAX_BEATS,
    SCORE_MAX_COUNT, SCORE_MAX_OFFSET_MS, SCORE_MAX_TEMPO, SCORE_MIN_TEMPO,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreBeat {
    /// Words of the reply the beat lands on as they are spoken. Without them
    /// the beat is placed `at_ms` after the score arrives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    pub at_ms: u32,
    /// Shifts a beat placed on words: early to prepare, late to follow through.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub offset_ms: i32,
    /// Targets reached from this moment and held until a later beat revises
    /// the same control, or for `hold_ms`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<BodyPose>,
    #[serde(default, rename = "move", skip_serializing_if = "Option::is_none")]
    pub motion: Option<ScoreMove>,
}

/// A movement that goes and comes back by itself, such as a nod.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreMove {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    pub amount: f32,
    pub count: u32,
    pub tempo: f32,
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}

/// The beats of a director's score that this reply and this body can play,
/// in their order. A beat on words needs those words in `response` (when it
/// is known) after the previous beat's words; a beat whose pose names an
/// unknown control, or whose move is unknown, is dropped whole. With
/// `capabilities`, controls the body cannot drive are left out and a beat
/// left with nothing to do is dropped.
pub fn grounded_score(
    value: &Value,
    response: Option<&str>,
    capabilities: Option<&[String]>,
) -> Vec<ScoreBeat> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    let mut beats = Vec::new();
    let mut cursor = 0usize;
    for item in items.iter().take(SCORE_MAX_BEATS as usize) {
        let Some(mut beat) = sanitize_beat(item) else {
            continue;
        };
        if let Some(text) = beat.text.as_deref() {
            match response {
                Some(response) => {
                    // Words repeat; a later beat on the same words means the next time they are said.
                    let Some(found) = response[cursor..].find(text) else {
                        continue;
                    };
                    cursor += found + text.chars().next().map_or(0, char::len_utf8);
                }
                None => continue,
            }
        }
        if let Some(capabilities) = capabilities
            && !restrict_to_capabilities(&mut beat, capabilities)
        {
            continue;
        }
        beats.push(beat);
    }
    beats
}

fn sanitize_beat(item: &Value) -> Option<ScoreBeat> {
    let object = item.as_object()?;
    let text = match object.get("text") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => {
            let count = text.chars().count();
            if count == 0 || count > SCORE_MAX_ANCHOR_CHARS as usize || text.trim() != text {
                return None;
            }
            Some(text.clone())
        }
        Some(_) => return None,
    };
    let at_ms = match object.get("atMs") {
        None | Some(Value::Null) => 0,
        Some(value) => value.as_u64()?.min(SCORE_MAX_AT_MS as u64) as u32,
    };
    let offset_ms = match object.get("offsetMs") {
        None | Some(Value::Null) => 0,
        Some(value) => {
            let limit = SCORE_MAX_OFFSET_MS as i64;
            value.as_i64()?.clamp(-limit, limit) as i32
        }
    };
    let pose = match object.get("pose") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let pose = sanitize_body_pose(value)?;
            (!pose.targets.is_empty()).then_some(pose)
        }
    };
    let motion = match object.get("move") {
        None | Some(Value::Null) => None,
        Some(value) => Some(sanitize_move(value)?),
    };
    if pose.is_none() && motion.is_none() {
        return None;
    }
    Some(ScoreBeat {
        text,
        at_ms: if object.get("text").is_some_and(|value| !value.is_null()) {
            0
        } else {
            at_ms
        },
        offset_ms,
        pose,
        motion,
    })
}

fn sanitize_move(value: &Value) -> Option<ScoreMove> {
    let object = value.as_object()?;
    let kind = object.get("kind")?.as_str()?;
    PERFORMANCE_SCORE_MOVES
        .iter()
        .find(|(name, _, _)| *name == kind)?;
    let word = |key: &str, allowed: &[&str]| -> Option<Option<String>> {
        match object.get(key) {
            None | Some(Value::Null) => Some(None),
            Some(Value::String(value)) if allowed.contains(&value.as_str()) => {
                Some(Some(value.clone()))
            }
            Some(_) => None,
        }
    };
    let number = |key: &str, default: f64| -> Option<f64> {
        match object.get(key) {
            None | Some(Value::Null) => Some(default),
            Some(value) => value.as_f64().filter(|value| value.is_finite()),
        }
    };
    Some(ScoreMove {
        kind: kind.to_owned(),
        side: word("side", PERFORMANCE_SCORE_SIDES)?,
        direction: word("direction", PERFORMANCE_SCORE_DIRECTIONS)?,
        amount: number("amount", 0.6)?.clamp(0.1, 1.0) as f32,
        count: number("count", 1.0)?
            .round()
            .clamp(1.0, SCORE_MAX_COUNT as f64) as u32,
        tempo: number("tempo", 1.0)?.clamp(SCORE_MIN_TEMPO as f64, SCORE_MAX_TEMPO as f64) as f32,
    })
}

/// Leaves out what this body cannot drive; false when nothing is left.
fn restrict_to_capabilities(beat: &mut ScoreBeat, capabilities: &[String]) -> bool {
    let drivable = |control: &str| {
        PERFORMANCE_BODY_CONTROLS
            .iter()
            .find(|entry| entry.0 == control)
            .is_some_and(|entry| capabilities.iter().any(|capability| capability == entry.3))
    };
    if let Some(pose) = beat.pose.as_mut() {
        pose.targets.retain(|control, _| drivable(control));
        if pose.targets.is_empty() {
            beat.pose = None;
        }
    }
    if let Some(motion) = beat.motion.as_ref() {
        let playable = PERFORMANCE_SCORE_MOVES
            .iter()
            .find(|(name, _, _)| *name == motion.kind)
            .is_some_and(|(_, controls, _)| controls.iter().any(|control| drivable(control)));
        if !playable {
            beat.motion = None;
        }
    }
    beat.pose.is_some() || beat.motion.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn beats_land_on_the_replys_words_in_order_or_on_time() {
        let response = "嗯嗯，我知道。不过嗯嗯，这次不一样。";
        let beats = grounded_score(
            &json!([
                {"text": "嗯嗯", "move": {"kind": "nod", "count": 2}},
                {"text": "不过", "offsetMs": -200, "pose": {"targets": {"headTilt": 0.4}, "transitionMs": 300, "holdMs": 0}},
                {"text": "嗯嗯", "move": {"kind": "shake"}},
                {"text": "没说过的话", "move": {"kind": "nod"}},
                {"atMs": 1200, "move": {"kind": "glance", "direction": "up-left"}}
            ]),
            Some(response),
            None,
        );
        assert_eq!(
            beats.len(),
            4,
            "a beat on words she does not say is dropped"
        );
        assert_eq!(beats[0].motion.as_ref().unwrap().count, 2);
        assert_eq!(beats[1].offset_ms, -200);
        // The second 嗯嗯 is the one after 不过.
        assert_eq!(beats[2].motion.as_ref().unwrap().kind, "shake");
        assert_eq!(beats[3].text, None);
        assert_eq!(beats[3].at_ms, 1200);
        // Without a reply (listening, idle) only timed beats can play.
        let idle = grounded_score(
            &json!([{"text": "嗯", "move": {"kind": "nod"}}, {"atMs": 400, "move": {"kind": "sigh"}}]),
            None,
            None,
        );
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0].motion.as_ref().unwrap().kind, "sigh");
    }

    #[test]
    fn a_beat_is_bounded_and_whole_or_dropped() {
        let beats = grounded_score(
            &json!([
                {"atMs": 999999, "move": {"kind": "nod", "count": 9, "amount": 3, "tempo": 0.1}},
                {"move": {"kind": "moonwalk"}},
                {"pose": {"targets": {"tailWag": 1}, "transitionMs": 300, "holdMs": 0}},
                {"move": {"kind": "wink", "side": "middle"}},
                {"atMs": 10},
                {"text": "  有空格", "move": {"kind": "nod"}}
            ]),
            Some("有空格"),
            None,
        );
        assert_eq!(beats.len(), 1);
        let motion = beats[0].motion.as_ref().unwrap();
        assert_eq!(beats[0].at_ms, SCORE_MAX_AT_MS);
        assert_eq!(motion.count, SCORE_MAX_COUNT);
        assert_eq!(motion.amount, 1.0);
        assert_eq!(motion.tempo, SCORE_MIN_TEMPO);
    }

    #[test]
    fn a_body_plays_only_what_it_can_drive() {
        let capabilities = vec!["head-body".to_string()];
        let beats = grounded_score(
            &json!([
                {"pose": {"targets": {"headTurn": 0.3, "leftArmRaise": 0.5}, "transitionMs": 300, "holdMs": 0}},
                {"move": {"kind": "beat", "side": "left"}},
                {"move": {"kind": "shrug"}}
            ]),
            None,
            Some(&capabilities),
        );
        assert_eq!(
            beats.len(),
            2,
            "a hand beat without arms has nothing to move"
        );
        assert_eq!(beats[0].pose.as_ref().unwrap().targets.len(), 1);
        // A shrug still lifts the chest and brows without arms.
        assert_eq!(beats[1].motion.as_ref().unwrap().kind, "shrug");
    }
}
