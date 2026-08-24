pub const MOOD_FLOOR: f64 = 10.0;
const DEFAULT_MOOD: f64 = 70.0;
const MAX_STEP: f64 = 10.0;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MoodTransition {
    pub before: f64,
    pub after: f64,
    pub band_before: String,
    pub band_after: String,
    pub delta: f64,
    pub cause: String,
    pub revision: i64,
}

pub fn clamp_mood(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 100.0)
    } else {
        DEFAULT_MOOD
    }
}

pub fn is_extremely_low(mood: f64) -> bool {
    clamp_mood(mood) <= MOOD_FLOOR
}

pub fn mood_band(mood: f64) -> &'static str {
    let mood = clamp_mood(mood);
    if mood <= MOOD_FLOOR {
        "floor"
    } else if mood < 40.0 {
        "low"
    } else if mood >= 85.0 {
        "high"
    } else {
        "normal"
    }
}

fn apply_delta(mood: f64, delta: f64) -> f64 {
    clamp_mood(mood + delta.clamp(-MAX_STEP, MAX_STEP))
}

pub fn apply_user_utterance(
    mood: f64,
    utterance_index_in_session: u32,
    praised: bool,
    scolded: bool,
    first_today: bool,
    gap_hours: f64,
) -> f64 {
    let mut delta = 2.0 - 0.5 * f64::from(utterance_index_in_session);
    if delta < 0.5 {
        delta = 0.5;
    }
    if praised {
        delta += 4.0;
    }
    if scolded {
        delta -= 8.0;
    }
    if first_today {
        delta += 3.0;
    }
    if gap_hours >= 12.0 {
        delta -= 4.0;
    }
    apply_delta(mood, delta)
}

pub fn apply_task_outcome(mood: f64, success: bool) -> f64 {
    apply_delta(mood, if success { 4.0 } else { -5.0 })
}

pub fn apply_departure(mood: f64) -> f64 {
    apply_delta(mood, -2.0)
}

/// After the chatting window (90s) and before the 12h gap rule.
///
/// `departure_after_message_secs` is how long after the last utterance the previous
/// departure was charged. Non-negative means this silence window already paid, so it
/// is skipped. This deliberately does not read `updated_at`: activity marks and
/// proactive stamps touch that constantly, which would silently disable the decay.
pub fn should_apply_departure(
    silent_secs: Option<i64>,
    departure_after_message_secs: Option<i64>,
) -> bool {
    let Some(silent) = silent_secs else {
        return false;
    };
    silent >= 90 && silent < 12 * 3600 && departure_after_message_secs.is_none_or(|since| since < 0)
}

/// How long a non-idle `activity` is believed. Nothing clears the row if the
/// process is killed between `working` and `idle`, and `working` is a gate in
/// `decide_ingest` — without this the addressee would go permanently silent
/// unless they happen to chat again. Far longer than any single turn.
pub const ACTIVITY_STALE_SECS: i64 = 30 * 60;

/// Reads through a stale activity. `age_secs` comes from `updated_at`, which
/// other writes also touch, so this can only ever be generous, never early.
pub fn effective_activity(activity: &str, age_secs: i64) -> &str {
    if activity == "idle" || age_secs < ACTIVITY_STALE_SECS {
        activity
    } else {
        "idle"
    }
}

pub fn parse_mood_hint(raw: &str) -> Option<f64> {
    let trimmed = raw
        .trim()
        .trim_matches(|c| c == '"' || c == '“' || c == '”')
        .split_whitespace()
        .next()?;
    let value: f64 = trimmed.parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    Some(value.clamp(-2.0, 2.0))
}

pub fn apply_mood_hint(mood: f64, hint: f64) -> f64 {
    apply_delta(mood, hint.clamp(-2.0, 2.0))
}

pub fn detect_mood_cue(text: &str) -> (bool, bool) {
    let lower = text.to_lowercase();
    let scolded = [
        "滚",
        "闭嘴",
        "滚开",
        "烦死",
        "讨厌你",
        "stupid",
        "shut up",
        "fuck you",
        "去死",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if scolded {
        return (false, true);
    }
    let praised = [
        "谢谢",
        "感谢",
        "辛苦了",
        "真棒",
        "太好了",
        "喜欢你",
        "thank you",
        "thanks",
        "good job",
        "love you",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    (praised, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extreme_low_includes_floor() {
        assert!(is_extremely_low(10.0));
        assert!(!is_extremely_low(10.1));
        assert_eq!(mood_band(10.0), "floor");
        assert_eq!(mood_band(39.9), "low");
        assert_eq!(mood_band(40.0), "normal");
        assert_eq!(mood_band(85.0), "high");
    }

    #[test]
    fn praise_lifts_and_clamp_holds() {
        let next = apply_user_utterance(95.0, 0, true, false, true, 0.0);
        assert!(next <= 100.0);
        assert!(next > 95.0);
    }

    #[test]
    fn long_gap_then_talk_does_not_crash() {
        let next = apply_user_utterance(70.0, 0, false, false, false, 20.0);
        assert!((next - 68.0).abs() < f64::EPSILON);
    }

    #[test]
    fn task_failure_lowers_mood() {
        assert_eq!(apply_task_outcome(70.0, false), 65.0);
    }

    #[test]
    fn mood_cue_prefers_scold_over_praise() {
        assert_eq!(detect_mood_cue("谢谢你还是滚吧"), (false, true));
        assert_eq!(detect_mood_cue("谢谢你今天帮我"), (true, false));
        assert_eq!(detect_mood_cue("今天天气不错"), (false, false));
    }

    #[test]
    fn departure_applies_once_after_chat_window() {
        assert!(!should_apply_departure(None, None));
        // Still inside the chat window.
        assert!(!should_apply_departure(Some(30), None));
        // Silent long enough and never charged yet.
        assert!(should_apply_departure(Some(120), None));
        // Already charged for this silence window.
        assert!(!should_apply_departure(Some(120), Some(4)));
        // Last charge predates the newest utterance, so this window is unpaid.
        assert!(should_apply_departure(Some(120), Some(-30)));
        // A long turn no longer disables the decay: only a real departure write does.
        assert!(should_apply_departure(Some(600), Some(-1)));
        assert!(!should_apply_departure(Some(13 * 3600), None));
        assert_eq!(apply_departure(70.0), 68.0);
    }

    #[test]
    fn stale_activity_reads_as_idle() {
        assert_eq!(effective_activity("working", 5), "working");
        assert_eq!(
            effective_activity("thinking", ACTIVITY_STALE_SECS - 1),
            "thinking"
        );
        // A process killed mid-task must not gate this person forever.
        assert_eq!(effective_activity("working", ACTIVITY_STALE_SECS), "idle");
        assert_eq!(effective_activity("talking", 10 * 3600), "idle");
        assert_eq!(effective_activity("idle", 10 * 3600), "idle");
    }

    #[test]
    fn mood_hint_parses_small_delta() {
        assert_eq!(parse_mood_hint(" 1 "), Some(1.0));
        assert_eq!(parse_mood_hint("\"-2\""), Some(-2.0));
        assert_eq!(parse_mood_hint("9"), Some(2.0));
        assert_eq!(parse_mood_hint("nope"), None);
        assert_eq!(apply_mood_hint(70.0, 1.0), 71.0);
    }
}
