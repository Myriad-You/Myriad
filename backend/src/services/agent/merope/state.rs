//! Per-addressee affect: mood (valence) × arousal, with a short-lived emotion layer.
//!
//! Silence regresses mood/arousal toward the persona set-point and emotion toward
//! origin. Conversation stamps settled_at so a live turn does not decay between
//! utterances. Numbers never belong in the speaking prompt.

pub const MOOD_FLOOR: f64 = 10.0;
pub const DEFAULT_MOOD: f64 = 70.0;
pub const DEFAULT_AROUSAL: f64 = 48.0;
pub const ORIGIN: f64 = 50.0;

const MAX_STEP: f64 = 10.0;
const TAU_MOOD_H: f64 = 72.0;
const TAU_AROUSAL_H: f64 = 36.0;
const TAU_EMOTION_H: f64 = 0.5;
const PULL: f64 = 0.22;
const PUSH: f64 = 0.05;
const DIMINISH_SPAN: f64 = 40.0;
const EMOTION_PULL_SKIP: f64 = 1.0;
const LITE_HINT_SCALE: f64 = 16.0;
/// Music is a restorative activity, not an unlimited reward. A qualified
/// listening block can ease low/neutral mood, but cannot by itself create the
/// character's happiest state.
pub const MUSIC_LISTENING_MIN_SECS: u32 = 10 * 60;
pub const MUSIC_LISTENING_MAX_SECS: u32 = 30 * 60;
pub const MUSIC_MOOD_CEILING: f64 = 80.0;

/// How long a non-idle `activity` is believed. Nothing clears the row if the
/// process is killed between `working` and `idle`, and `working` is a gate in
/// `decide_ingest` — without this the addressee would go permanently silent
/// unless they happen to chat again. Far longer than any single turn.
pub const ACTIVITY_STALE_SECS: i64 = 30 * 60;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affect {
    pub mood: f64,
    pub arousal: f64,
    pub emotion: f64,
    pub emotion_arousal: f64,
}

impl Affect {
    pub fn at_rest(base: AffectBaseline) -> Self {
        Self {
            mood: clamp(base.mood),
            arousal: clamp(base.arousal),
            emotion: ORIGIN,
            emotion_arousal: ORIGIN,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AffectBaseline {
    pub mood: f64,
    pub arousal: f64,
}

impl Default for AffectBaseline {
    fn default() -> Self {
        Self {
            mood: DEFAULT_MOOD,
            arousal: DEFAULT_AROUSAL,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Appraisal {
    pub emotion: f64,
    pub arousal: f64,
}

pub const APPRAISAL_PRAISE: Appraisal = Appraisal {
    emotion: 82.0,
    arousal: 58.0,
};
/// Below `MOOD_FLOOR` so repeated scolding can actually reach 极低.
/// 22 sat above the floor and made `is_extremely_low` a dead branch.
pub const APPRAISAL_SCOLD: Appraisal = Appraisal {
    emotion: 8.0,
    arousal: 70.0,
};
pub const APPRAISAL_TASK_OK: Appraisal = Appraisal {
    emotion: 80.0,
    arousal: 60.0,
};
pub const APPRAISAL_TASK_FAIL: Appraisal = Appraisal {
    emotion: 30.0,
    arousal: 64.0,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoodTransition {
    pub before: f64,
    pub after: f64,
    #[serde(default = "default_arousal_field")]
    pub arousal_before: f64,
    #[serde(default = "default_arousal_field")]
    pub arousal_after: f64,
    pub band_before: String,
    pub band_after: String,
    pub delta: f64,
    pub cause: String,
    pub revision: i64,
}

fn default_arousal_field() -> f64 {
    DEFAULT_AROUSAL
}

impl MoodTransition {
    pub fn from_affect(before: &Affect, after: &Affect, cause: &str, revision: i64) -> Self {
        Self {
            before: before.mood,
            after: after.mood,
            arousal_before: before.arousal,
            arousal_after: after.arousal,
            band_before: mood_band(before.mood, before.arousal).to_string(),
            band_after: mood_band(after.mood, after.arousal).to_string(),
            delta: after.mood - before.mood,
            cause: cause.to_string(),
            revision,
        }
    }
}

pub fn clamp_mood(value: f64) -> f64 {
    clamp(value)
}

pub fn clamp(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        ORIGIN
    }
}

pub fn is_extremely_low(mood: f64) -> bool {
    clamp(mood) <= MOOD_FLOOR
}

/// Circumplex band. Floor is valence only; the rest split on 55/55.
pub fn mood_band(mood: f64, arousal: f64) -> &'static str {
    let mood = clamp(mood);
    let arousal = clamp(arousal);
    if mood <= MOOD_FLOOR {
        "floor"
    } else if mood < 55.0 && arousal < 55.0 {
        "sad"
    } else if mood < 55.0 {
        "tense"
    } else if arousal < 55.0 {
        "calm"
    } else {
        "excited"
    }
}

pub fn regress(value: f64, base: f64, dt_hours: f64, tau_hours: f64) -> f64 {
    if !dt_hours.is_finite() || dt_hours <= 0.0 || !tau_hours.is_finite() || tau_hours <= 0.0 {
        return clamp(value);
    }
    clamp(base + (value - base) * (-dt_hours / tau_hours).exp())
}

pub fn settle(affect: Affect, base: AffectBaseline, mood_hours: f64, emotion_hours: f64) -> Affect {
    Affect {
        mood: regress(affect.mood, base.mood, mood_hours, TAU_MOOD_H),
        arousal: regress(affect.arousal, base.arousal, mood_hours, TAU_AROUSAL_H),
        emotion: regress(affect.emotion, ORIGIN, emotion_hours, TAU_EMOTION_H),
        emotion_arousal: regress(affect.emotion_arousal, ORIGIN, emotion_hours, TAU_EMOTION_H),
    }
}

fn diminish(value: f64, delta: f64) -> f64 {
    if delta == 0.0 {
        return 0.0;
    }
    let headroom = if delta > 0.0 { 100.0 - value } else { value };
    delta * (headroom / DIMINISH_SPAN).clamp(0.0, 1.0)
}

pub fn repeat_scale(utterance_index: u32) -> f64 {
    1.0 / (1.0 + 0.25 * f64::from(utterance_index))
}

fn appraisal_pull(value: f64, emotion: f64) -> f64 {
    let delta = PULL * (emotion - value);
    // Appraisal is a signed influence around ORIGIN, not a replacement mood.
    // A mild compliment must not punish an already happy character; calming
    // someone already calmer than the target must not excite them either.
    let directional = if emotion > ORIGIN {
        delta.max(0.0)
    } else {
        delta.min(0.0)
    };
    diminish(value, directional.clamp(-MAX_STEP, MAX_STEP))
}

pub fn pull_push(affect: &mut Affect) {
    // Each axis can be neutral independently: calming down does not imply a
    // change of affection, and affection need not imply excitement.
    if (affect.emotion - ORIGIN).abs() >= EMOTION_PULL_SKIP {
        let dm = appraisal_pull(affect.mood, affect.emotion);
        affect.mood = clamp(affect.mood + dm);
        affect.emotion = clamp(affect.emotion + PUSH * (affect.mood - affect.emotion));
    }
    if (affect.emotion_arousal - ORIGIN).abs() >= EMOTION_PULL_SKIP {
        let da = appraisal_pull(affect.arousal, affect.emotion_arousal);
        affect.arousal = clamp(affect.arousal + da);
        affect.emotion_arousal =
            clamp(affect.emotion_arousal + PUSH * (affect.arousal - affect.emotion_arousal));
    }
}

pub fn apply_appraisal(affect: &mut Affect, appraisal: Appraisal, scale: f64) {
    let scale = if scale.is_finite() {
        scale.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let target_e = ORIGIN + (appraisal.emotion - ORIGIN) * scale;
    let target_a = ORIGIN + (appraisal.arousal - ORIGIN) * scale;
    affect.emotion = clamp(target_e);
    affect.emotion_arousal = clamp(target_a);
    pull_push(affect);
}

pub fn apply_user_utterance(
    affect: &mut Affect,
    utterance_index_in_session: u32,
    praised: bool,
    scolded: bool,
) {
    let scale = repeat_scale(utterance_index_in_session);
    if scolded {
        apply_appraisal(affect, APPRAISAL_SCOLD, scale);
    } else if praised {
        apply_appraisal(affect, APPRAISAL_PRAISE, scale);
    }
}

pub fn apply_task_outcome(affect: &mut Affect, success: bool) {
    apply_appraisal(
        affect,
        if success {
            APPRAISAL_TASK_OK
        } else {
            APPRAISAL_TASK_FAIL
        },
        1.0,
    );
}

/// Apply one server-qualified block of actual music playback.
///
/// We deliberately leave arousal and the short emotion untouched: without
/// knowing whether the track is calming, sad or energetic, changing either
/// would invent a reaction. The slow valence nudge represents the modest
/// restorative benefit of choosing to listen, with duration saturation,
/// ordinary headroom diminishing and a comfort ceiling.
pub fn apply_music_listening(affect: &mut Affect, listened_seconds: u32) {
    if listened_seconds < MUSIC_LISTENING_MIN_SECS || affect.mood >= MUSIC_MOOD_CEILING {
        return;
    }
    let seconds = listened_seconds.min(MUSIC_LISTENING_MAX_SECS);
    let duration_span = MUSIC_LISTENING_MAX_SECS - MUSIC_LISTENING_MIN_SECS;
    let duration = f64::from(seconds - MUSIC_LISTENING_MIN_SECS) / f64::from(duration_span);
    let raw_boost = 1.5 + 1.5 * duration;
    let boost = diminish(affect.mood, raw_boost).min(MUSIC_MOOD_CEILING - affect.mood);
    affect.mood = clamp(affect.mood + boost);
}

pub fn lite_appraisal(valence: i32, arousal: i32) -> Appraisal {
    Appraisal {
        emotion: ORIGIN + LITE_HINT_SCALE * f64::from(valence.clamp(-2, 2)),
        arousal: ORIGIN + LITE_HINT_SCALE * f64::from(arousal.clamp(-2, 2)),
    }
}

/// Reads through a stale activity. `age_secs` comes from `updated_at`, which
/// other writes also touch, so this can only ever be generous, never early.
pub fn effective_activity(activity: &str, age_secs: i64) -> &str {
    if activity == "idle" || age_secs < ACTIVITY_STALE_SECS {
        activity
    } else {
        "idle"
    }
}

const INTROVERT: &[&str] = &[
    "克制",
    "慢热",
    "内向",
    "quiet",
    "reserved",
    "shy",
    "restrained",
];
const EXTRAVERT: &[&str] = &[
    "活泼", "开放", "外向", "bright", "playful", "open", "cheerful",
];

pub fn persona_affect_baseline(
    persona_json: Option<&serde_json::Value>,
    personality: &str,
) -> AffectBaseline {
    let temperament = persona_json
        .and_then(|value| value.get("temperament"))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    let social = persona_json
        .and_then(|value| value.get("socialStyle"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let blob = format!("{temperament} {social} {personality}").to_lowercase();
    let mut base = AffectBaseline::default();
    if contains_any(&blob, INTROVERT) {
        base.arousal = 42.0;
    } else if contains_any(&blob, EXTRAVERT) {
        base.arousal = 56.0;
    }
    base
}

fn contains_any(blob: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| blob.contains(needle))
}

pub fn detect_mood_cue(text: &str) -> (bool, bool) {
    // Only unambiguous standalone address gets a synchronous mood change.
    // Mixed sentiment, quoted/code text, reported speech and negation need
    // context; substring matching here would bypass the appraiser entirely.
    let normalized = text
        .trim()
        .trim_end_matches(['!', '！', '.', '。', '~', '～'])
        .trim();
    let lower = normalized.to_lowercase();
    let praised = matches!(
        lower.as_str(),
        "谢谢"
            | "谢谢你"
            | "感谢你"
            | "辛苦了"
            | "你真棒"
            | "喜欢你"
            | "我喜欢你"
            | "thank you"
            | "thanks"
            | "good job"
            | "love you"
            | "i love you"
            | "ありがとう"
            | "ありがとうございます"
    );
    let scolded = matches!(
        lower.as_str(),
        "滚" | "闭嘴" | "滚开" | "讨厌你" | "我讨厌你" | "你真笨" | "shut up" | "fuck you" | "去死"
    );
    (praised, scolded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rest() -> Affect {
        Affect::at_rest(AffectBaseline::default())
    }

    #[test]
    fn extreme_low_includes_floor() {
        assert!(is_extremely_low(10.0));
        assert!(!is_extremely_low(10.1));
        assert_eq!(mood_band(10.0, 48.0), "floor");
        assert_eq!(mood_band(30.0, 40.0), "sad");
        assert_eq!(mood_band(30.0, 70.0), "tense");
        assert_eq!(mood_band(70.0, 48.0), "calm");
        assert_eq!(mood_band(90.0, 48.0), "calm");
        assert_eq!(mood_band(90.0, 70.0), "excited");
    }

    #[test]
    fn praise_lifts_mood_from_baseline() {
        let mut affect = rest();
        apply_user_utterance(&mut affect, 0, true, false);
        assert!(affect.mood > DEFAULT_MOOD);
        assert!(affect.mood <= 100.0);
        assert!(affect.emotion > ORIGIN);
    }

    #[test]
    fn repeated_praise_shrinks() {
        let mut first = rest();
        apply_user_utterance(&mut first, 0, true, false);
        let mut second = rest();
        apply_user_utterance(&mut second, 1, true, false);
        assert!(first.mood - DEFAULT_MOOD > second.mood - DEFAULT_MOOD);
    }

    #[test]
    fn silence_and_return_do_not_move_mood() {
        let mut affect = rest();
        apply_user_utterance(&mut affect, 0, false, false);
        assert_eq!(affect, rest());
    }

    #[test]
    fn task_failure_lowers_mood() {
        let mut affect = rest();
        apply_task_outcome(&mut affect, false);
        assert!(affect.mood < DEFAULT_MOOD);
    }

    #[test]
    fn repeated_scold_can_reach_the_mood_floor() {
        let mut affect = rest();
        for _ in 0..200 {
            apply_user_utterance(&mut affect, 0, false, true);
            if is_extremely_low(affect.mood) {
                assert_eq!(mood_band(affect.mood, affect.arousal), "floor");
                return;
            }
        }
        panic!("mood {} never reached the floor", affect.mood);
    }

    #[test]
    fn repeated_success_converges_below_the_ceiling() {
        let mut affect = rest();
        for _ in 0..10_000 {
            apply_task_outcome(&mut affect, true);
        }
        assert!(affect.mood > DEFAULT_MOOD);
        assert!(affect.mood < 100.0);
        let settled = affect.mood;
        apply_task_outcome(&mut affect, true);
        assert!((affect.mood - settled).abs() < 0.05);
    }

    #[test]
    fn repeated_failure_converges_above_zero() {
        let mut affect = rest();
        for _ in 0..10_000 {
            apply_task_outcome(&mut affect, false);
        }
        assert!(affect.mood < DEFAULT_MOOD);
        assert!(affect.mood > 0.0);
        assert!(!is_extremely_low(affect.mood));
        let settled = affect.mood;
        apply_task_outcome(&mut affect, false);
        assert!((affect.mood - settled).abs() < 0.05);
    }

    #[test]
    fn qualified_music_listening_modestly_lifts_mood_without_inventing_arousal() {
        let mut affect = Affect {
            mood: 40.0,
            arousal: 67.0,
            emotion: 21.0,
            emotion_arousal: 73.0,
        };
        apply_music_listening(&mut affect, MUSIC_LISTENING_MIN_SECS);
        assert_eq!(affect.mood, 41.5);
        assert_eq!(affect.arousal, 67.0);
        assert_eq!(affect.emotion, 21.0);
        assert_eq!(affect.emotion_arousal, 73.0);
    }

    #[test]
    fn music_listening_requires_real_duration_and_has_a_comfort_ceiling() {
        let mut too_short = rest();
        apply_music_listening(&mut too_short, MUSIC_LISTENING_MIN_SECS - 1);
        assert_eq!(too_short, rest());

        let mut affect = Affect {
            mood: MUSIC_MOOD_CEILING - 0.25,
            ..rest()
        };
        apply_music_listening(&mut affect, MUSIC_LISTENING_MAX_SECS * 10);
        assert_eq!(affect.mood, MUSIC_MOOD_CEILING);
        apply_music_listening(&mut affect, MUSIC_LISTENING_MAX_SECS);
        assert_eq!(affect.mood, MUSIC_MOOD_CEILING);
    }

    #[test]
    fn instant_mood_cues_require_direct_standalone_address() {
        assert_eq!(detect_mood_cue(" 谢谢你！！ "), (true, false));
        assert_eq!(detect_mood_cue("闭嘴！"), (false, true));
        assert_eq!(detect_mood_cue("Thank You!"), (true, false));
        for text in [
            "谢谢你还是滚吧",
            "谢谢你今天帮我",
            "不是讨厌你",
            "不喜欢你",
            "不要说谢谢",
            "他说闭嘴",
            "他说：喜欢你",
            "“闭嘴”",
            "`thanks`",
            "代码滚动有点慢",
            "this stupid bug",
            "I don't love you",
            "谢谢你？",
        ] {
            assert_eq!(detect_mood_cue(text), (false, false), "{text}");
        }
    }

    #[test]
    fn stale_activity_reads_as_idle() {
        assert_eq!(effective_activity("working", 5), "working");
        assert_eq!(
            effective_activity("thinking", ACTIVITY_STALE_SECS - 1),
            "thinking"
        );
        assert_eq!(effective_activity("working", ACTIVITY_STALE_SECS), "idle");
        assert_eq!(effective_activity("talking", 10 * 3600), "idle");
        assert_eq!(effective_activity("idle", 10 * 3600), "idle");
    }

    #[test]
    fn appraisal_axes_can_change_independently() {
        let mut affect = rest();
        apply_appraisal(&mut affect, lite_appraisal(0, -2), 1.0);
        assert_eq!(affect.mood, DEFAULT_MOOD);
        assert!(affect.arousal < DEFAULT_AROUSAL);
        let mut affect = rest();
        apply_appraisal(&mut affect, lite_appraisal(2, 0), 1.0);
        assert!(affect.mood > DEFAULT_MOOD);
        assert_eq!(affect.arousal, DEFAULT_AROUSAL);
    }

    #[test]
    fn signed_appraisal_never_moves_either_axis_in_the_opposite_direction() {
        for initial in 0..=100 {
            for valence in -2..=2 {
                for arousal in -2..=2 {
                    let mut affect = Affect::at_rest(AffectBaseline {
                        mood: f64::from(initial),
                        arousal: f64::from(initial),
                    });
                    apply_appraisal(&mut affect, lite_appraisal(valence, arousal), 1.0);
                    for (next, sign) in [(affect.mood, valence), (affect.arousal, arousal)] {
                        let delta = next - f64::from(initial);
                        assert!(
                            match sign.cmp(&0) {
                                std::cmp::Ordering::Greater => delta >= 0.0,
                                std::cmp::Ordering::Less => delta <= 0.0,
                                std::cmp::Ordering::Equal => delta == 0.0,
                            },
                            "initial={initial}, valence={valence}, arousal={arousal}, delta={delta}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn habituated_local_praise_does_not_lower_an_already_happy_mood() {
        for utterance in [0, 1, 10, 100] {
            let mut affect = Affect::at_rest(AffectBaseline {
                mood: 85.0,
                arousal: 15.0,
            });
            apply_user_utterance(&mut affect, utterance, true, false);
            assert!(affect.mood >= 85.0);
            let mut affect = Affect::at_rest(AffectBaseline {
                mood: 5.0,
                arousal: 95.0,
            });
            apply_user_utterance(&mut affect, utterance, false, true);
            assert!(affect.mood <= 5.0);
            assert!(affect.arousal >= 95.0);
        }
    }

    #[test]
    fn origin_emotion_does_not_pull_mood() {
        let mut affect = rest();
        affect.emotion = ORIGIN;
        affect.emotion_arousal = ORIGIN;
        let before = affect;
        pull_push(&mut affect);
        assert_eq!(affect, before);
    }

    #[test]
    fn ninety_days_idle_returns_to_persona_set_point() {
        let mut affect = Affect {
            mood: 20.0,
            arousal: 90.0,
            emotion: 95.0,
            emotion_arousal: 95.0,
        };
        let base = AffectBaseline::default();
        let dt = 90.0 * 24.0;
        affect = settle(affect, base, dt, dt);
        assert!((affect.mood - base.mood).abs() < 0.05);
        assert!((affect.arousal - base.arousal).abs() < 0.05);
        assert!((affect.emotion - ORIGIN).abs() < 0.05);
        assert!((affect.emotion_arousal - ORIGIN).abs() < 0.05);
    }

    #[test]
    fn persona_set_point_follows_temperament() {
        let quiet = persona_affect_baseline(Some(&serde_json::json!({"socialStyle": "内向"})), "");
        assert!((quiet.arousal - 42.0).abs() < f64::EPSILON);
        assert!((quiet.mood - DEFAULT_MOOD).abs() < f64::EPSILON);
        let bright =
            persona_affect_baseline(Some(&serde_json::json!({"temperament": ["活泼"]})), "");
        assert!((bright.arousal - 56.0).abs() < f64::EPSILON);
        assert_eq!(persona_affect_baseline(None, "").arousal, DEFAULT_AROUSAL);
    }

    #[test]
    fn lite_hint_maps_onto_the_origin() {
        let appraisal = lite_appraisal(2, -2);
        assert!((appraisal.emotion - 82.0).abs() < f64::EPSILON);
        assert!((appraisal.arousal - 18.0).abs() < f64::EPSILON);
    }
}
