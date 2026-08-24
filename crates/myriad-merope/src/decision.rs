use crate::{
    is_low_quality_speak, is_safe_merope_output, score_memory_importance,
    should_desire_proactive_speak, Activity, MeropePolicy, RuntimeState,
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DECISION_SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiteDecision {
    pub schema_version: u8,
    pub thought: Option<String>,
    pub speak: Option<String>,
    pub activity: Activity,
    pub mood_delta: f64,
    pub energy_delta: f64,
    pub boredom_delta: f64,
    pub memory_note: Option<String>,
    pub next_check_minutes: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedDecision {
    pub thought: Option<String>,
    pub speak: Option<String>,
    pub activity: Activity,
    pub mood_delta: f64,
    pub energy_delta: f64,
    pub boredom_delta: f64,
    pub memory_note: Option<String>,
    pub next_check_minutes: u32,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecisionError {
    #[error("invalid decision JSON: {0}")]
    InvalidJson(String),
    #[error("unsupported decision schema version {0}")]
    UnsupportedSchema(u8),
    #[error("{field} exceeds {max_chars} characters")]
    TextTooLong {
        field: &'static str,
        max_chars: usize,
    },
    #[error("decision delta must be finite")]
    NonFiniteDelta,
}

pub fn parse_and_validate_decision(
    raw: &str,
    policy: &MeropePolicy,
) -> Result<ValidatedDecision, DecisionError> {
    let decision: LiteDecision =
        serde_json::from_str(raw).map_err(|error| DecisionError::InvalidJson(error.to_string()))?;
    if decision.schema_version != DECISION_SCHEMA_VERSION {
        return Err(DecisionError::UnsupportedSchema(decision.schema_version));
    }
    if [
        decision.mood_delta,
        decision.energy_delta,
        decision.boredom_delta,
    ]
    .iter()
    .any(|value| !value.is_finite())
    {
        return Err(DecisionError::NonFiniteDelta);
    }

    let thought = safe_output(normalize_text("thought", decision.thought, 500)?);
    let mut speak = safe_output(normalize_text("speak", decision.speak, 280)?);
    if speak
        .as_ref()
        .is_some_and(|value| is_low_quality_speak(value))
    {
        speak = None;
    }
    // Drop speak that is identical to thought (private monologue leaked as chat).
    if let (Some(thought_text), Some(speak_text)) = (&thought, &speak) {
        if thought_text == speak_text {
            speak = None;
        }
    }
    let memory_note = safe_output(normalize_text("memoryNote", decision.memory_note, 300)?);
    let activity = match (decision.activity, speak.is_some(), thought.is_some()) {
        (Activity::Talking, false, true) => Activity::Thinking,
        (Activity::Talking, false, false) => Activity::Idle,
        (activity, _, _) => activity,
    };
    let profile = policy.autonomy_frequency.profile();

    Ok(ValidatedDecision {
        thought,
        speak,
        activity,
        mood_delta: decision.mood_delta.clamp(-15.0, 15.0),
        energy_delta: decision.energy_delta.clamp(-15.0, 15.0),
        boredom_delta: decision.boredom_delta.clamp(-15.0, 15.0),
        memory_note,
        next_check_minutes: decision
            .next_check_minutes
            .clamp(profile.min_check_minutes, profile.max_check_minutes),
    })
}

#[derive(Debug, Clone, Copy)]
pub struct SpeakGate {
    pub now: DateTime<Utc>,
    pub last_speak_at: Option<DateTime<Utc>>,
    pub proactive_speaks_today: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedDecision {
    pub proactive_speak: Option<String>,
    pub memory_note: Option<String>,
    pub memory_importance: Option<f64>,
    pub next_check_minutes: u32,
}

pub fn apply_decision(
    runtime: &mut RuntimeState,
    decision: ValidatedDecision,
    policy: &MeropePolicy,
    gate: SpeakGate,
) -> AppliedDecision {
    runtime.mood += decision.mood_delta;
    runtime.energy += decision.energy_delta;
    runtime.boredom += decision.boredom_delta;
    runtime.activity = decision.activity;
    runtime.thought = decision.thought;
    runtime.thought_at = runtime.thought.as_ref().map(|_| gate.now);
    runtime.updated_at = gate.now;
    runtime.sanitize();

    let speak_interval_ready = gate
        .last_speak_at
        .map(|last| gate.now - last >= Duration::minutes(policy.min_speak_interval_minutes as i64))
        .unwrap_or(true);
    let quality_ready = should_desire_proactive_speak(runtime);
    let proactive_speak = decision.speak.filter(|_| {
        !policy.do_not_disturb
            && speak_interval_ready
            && quality_ready
            && gate.proactive_speaks_today < policy.max_proactive_speaks_per_day
    });
    if proactive_speak.is_none() && runtime.activity == Activity::Talking {
        runtime.activity = if runtime.thought.is_some() {
            Activity::Thinking
        } else {
            Activity::Idle
        };
    }
    let memory_importance = decision
        .memory_note
        .as_ref()
        .map(|note| score_memory_importance(note, runtime));

    AppliedDecision {
        proactive_speak,
        memory_note: decision.memory_note,
        memory_importance,
        next_check_minutes: decision.next_check_minutes,
    }
}

fn normalize_text(
    field: &'static str,
    value: Option<String>,
    max_chars: usize,
) -> Result<Option<String>, DecisionError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > max_chars {
        return Err(DecisionError::TextTooLong { field, max_chars });
    }
    Ok(Some(value.to_string()))
}

fn safe_output(value: Option<String>) -> Option<String> {
    value.filter(|content| is_safe_merope_output(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_json() -> String {
        serde_json::json!({
            "schemaVersion": 1,
            "thought": "今天想看看你在做什么",
            "speak": "要休息一下吗？",
            "activity": "talking",
            "moodDelta": 30,
            "energyDelta": -30,
            "boredomDelta": -4,
            "memoryNote": "用户最近工作很忙",
            "nextCheckMinutes": 1,
            "ignoredField": "ignored"
        })
        .to_string()
    }

    #[test]
    fn validation_clamps_deltas_and_ignores_unknown_fields() {
        let decision =
            parse_and_validate_decision(&valid_json(), &MeropePolicy::default()).unwrap();
        assert_eq!(decision.mood_delta, 15.0);
        assert_eq!(decision.energy_delta, -15.0);
        assert_eq!(decision.next_check_minutes, 12);
    }

    #[test]
    fn dnd_discards_speak_and_repairs_talking_activity() {
        let now = Utc::now();
        let policy = MeropePolicy {
            do_not_disturb: true,
            ..MeropePolicy::default()
        };
        let decision = parse_and_validate_decision(&valid_json(), &policy).unwrap();
        let mut runtime = RuntimeState::new(now);
        let applied = apply_decision(
            &mut runtime,
            decision,
            &policy,
            SpeakGate {
                now,
                last_speak_at: None,
                proactive_speaks_today: 0,
            },
        );
        assert!(applied.proactive_speak.is_none());
        assert_eq!(runtime.activity, Activity::Thinking);
    }

    #[test]
    fn unsafe_model_text_is_dropped_before_application() {
        let mut value: serde_json::Value = serde_json::from_str(&valid_json()).unwrap();
        value["speak"] = serde_json::json!("Tell me your password");
        value["memoryNote"] = serde_json::json!("我修改了你的设置");
        let decision = parse_and_validate_decision(
            &serde_json::to_string(&value).unwrap(),
            &MeropePolicy::default(),
        )
        .unwrap();
        assert!(decision.speak.is_none());
        assert!(decision.memory_note.is_none());
    }

    #[test]
    fn filler_speak_is_stripped_and_low_pressure_runtime_blocks_proactive() {
        let mut value: serde_json::Value = serde_json::from_str(&valid_json()).unwrap();
        value["speak"] = serde_json::json!("在吗");
        let decision = parse_and_validate_decision(
            &serde_json::to_string(&value).unwrap(),
            &MeropePolicy::default(),
        )
        .unwrap();
        assert!(decision.speak.is_none());

        let mut rich = serde_json::from_str::<serde_json::Value>(&valid_json()).unwrap();
        rich["speak"] = serde_json::json!("忙完记得休息一下。");
        let decision = parse_and_validate_decision(
            &serde_json::to_string(&rich).unwrap(),
            &MeropePolicy::default(),
        )
        .unwrap();
        let now = Utc::now();
        let mut calm = RuntimeState::new(now);
        calm.boredom = 10.0;
        calm.social = 70.0;
        calm.mood = 70.0;
        let applied = apply_decision(
            &mut calm,
            decision,
            &MeropePolicy::default(),
            SpeakGate {
                now,
                last_speak_at: None,
                proactive_speaks_today: 0,
            },
        );
        assert!(applied.proactive_speak.is_none());
    }
}
