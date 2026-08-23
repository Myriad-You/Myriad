use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPerformancePlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<ChatPerformanceBaseline>,
    #[serde(default)]
    pub cues: Vec<ChatPerformanceCue>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPerformanceBaseline {
    pub expression: String,
    pub posture: String,
    pub motion_energy: f32,
    pub attention: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatPerformanceCue {
    pub intent: String,
    pub at_ms: u32,
    pub intensity: f32,
    pub tempo: f32,
    pub fade_in_ms: u32,
    pub fade_out_ms: u32,
    pub interrupt: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedChatPerformance {
    pub reply: String,
    pub plan: Option<ChatPerformancePlan>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEnvelope {
    reply: String,
    #[serde(default, rename = "performance")]
    _performance: Option<RawPlan>,
}

#[derive(Debug, Deserialize)]
struct RawPlan {
    #[serde(default)]
    baseline: Option<RawBaseline>,
    #[serde(default)]
    cues: Vec<RawCue>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawBaseline {
    expression: String,
    posture: String,
    #[serde(default = "default_motion_energy")]
    motion_energy: f32,
    #[serde(default = "default_attention")]
    attention: f32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCue {
    intent: String,
    #[serde(default)]
    at_ms: u32,
    #[serde(default = "default_intensity")]
    intensity: f32,
    #[serde(default = "default_tempo")]
    tempo: f32,
    #[serde(default = "default_fade_in")]
    fade_in_ms: u32,
    #[serde(default = "default_fade_out")]
    fade_out_ms: u32,
    #[serde(default = "default_interrupt")]
    interrupt: String,
}

pub fn parse_chat_performance(raw: &str) -> ParsedChatPerformance {
    let trimmed = raw.trim();
    let json = strip_json_fence(trimmed);
    let Ok(envelope) = serde_json::from_str::<RawEnvelope>(json) else {
        return ParsedChatPerformance {
            reply: trimmed.chars().take(2_000).collect(),
            plan: None,
        };
    };
    let reply = envelope
        .reply
        .trim()
        .chars()
        .take(2_000)
        .collect::<String>();
    if reply.is_empty() {
        return ParsedChatPerformance {
            reply: trimmed.chars().take(2_000).collect(),
            plan: None,
        };
    }
    // Semantic acting is exclusively selected by the strict-Lite motion
    // director. A reply model's embedded performance payload is ignored.
    ParsedChatPerformance { reply, plan: None }
}

/// Parses the Lite motion director's plan. This is deliberately separate from
/// `parse_chat_performance`: the motion model never gets to author the reply.
pub fn parse_performance_plan(raw: &str) -> Option<ChatPerformancePlan> {
    serde_json::from_str::<RawPlan>(strip_json_fence(raw.trim()))
        .ok()
        .and_then(sanitize_plan)
}

fn strip_json_fence(value: &str) -> &str {
    value
        .strip_prefix("```json")
        .or_else(|| value.strip_prefix("```"))
        .and_then(|inner| inner.strip_suffix("```"))
        .unwrap_or(value)
        .trim()
}

fn sanitize_plan(plan: RawPlan) -> Option<ChatPerformancePlan> {
    let baseline = plan.baseline.and_then(sanitize_baseline);
    let cues = plan
        .cues
        .into_iter()
        .take(3)
        .filter_map(sanitize_cue)
        .collect::<Vec<_>>();
    (baseline.is_some() || !cues.is_empty()).then_some(ChatPerformancePlan { baseline, cues })
}

fn sanitize_baseline(baseline: RawBaseline) -> Option<ChatPerformanceBaseline> {
    const EXPRESSIONS: &[&str] = &["withdrawn", "subdued", "steady", "warm"];
    const POSTURES: &[&str] = &["closed", "neutral", "open"];
    if !EXPRESSIONS.contains(&baseline.expression.as_str())
        || !POSTURES.contains(&baseline.posture.as_str())
        || !baseline.motion_energy.is_finite()
        || !baseline.attention.is_finite()
    {
        return None;
    }
    Some(ChatPerformanceBaseline {
        expression: baseline.expression,
        posture: baseline.posture,
        motion_energy: baseline.motion_energy.clamp(0.2, 1.4),
        attention: baseline.attention.clamp(0.0, 1.0),
    })
}

fn sanitize_cue(cue: RawCue) -> Option<ChatPerformanceCue> {
    const INTENTS: &[&str] = &[
        "greet",
        "respond",
        "question",
        "delight",
        "emphasize",
        "listen",
        "notify",
    ];
    const INTERRUPTS: &[&str] = &["replace", "queue", "if-lower"];
    if !INTENTS.contains(&cue.intent.as_str())
        || !cue.intensity.is_finite()
        || !cue.tempo.is_finite()
    {
        return None;
    }
    Some(ChatPerformanceCue {
        intent: cue.intent,
        at_ms: cue.at_ms.min(5_000),
        intensity: cue.intensity.clamp(0.2, 1.4),
        tempo: cue.tempo.clamp(0.5, 1.6),
        fade_in_ms: cue.fade_in_ms.clamp(40, 600),
        fade_out_ms: cue.fade_out_ms.clamp(60, 800),
        interrupt: if INTERRUPTS.contains(&cue.interrupt.as_str()) {
            cue.interrupt
        } else {
            default_interrupt()
        },
    })
}

fn default_intensity() -> f32 {
    1.0
}

fn default_tempo() -> f32 {
    1.0
}

fn default_fade_in() -> u32 {
    150
}

fn default_fade_out() -> u32 {
    220
}

fn default_interrupt() -> String {
    "if-lower".to_string()
}

fn default_motion_energy() -> f32 {
    1.0
}

fn default_attention() -> f32 {
    0.8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_parser_ignores_non_lite_performance() {
        let parsed = parse_chat_performance(
            r#"{"reply":"Hello!","performance":{"cues":[{"intent":"delight","atMs":9000,"intensity":9,"tempo":0.1,"interrupt":"unsafe"}]}}"#,
        );
        assert_eq!(parsed.reply, "Hello!");
        assert!(parsed.plan.is_none());

        let plan = parse_performance_plan(
            r#"{"cues":[{"intent":"delight","atMs":9000,"intensity":9,"tempo":0.1,"interrupt":"unsafe"}]}"#,
        )
        .unwrap();
        let cue = &plan.cues[0];
        assert_eq!(cue.intent, "delight");
        assert_eq!(cue.at_ms, 5_000);
        assert_eq!(cue.intensity, 1.4);
        assert_eq!(cue.tempo, 0.5);
        assert_eq!(cue.interrupt, "if-lower");
    }

    #[test]
    fn parses_and_bounds_lite_selected_baseline() {
        let plan = parse_performance_plan(
            r#"{"baseline":{"expression":"warm","posture":"open","motionEnergy":9,"attention":-2},"cues":[]}"#,
        )
        .unwrap();
        let baseline = plan.baseline.unwrap();
        assert_eq!(baseline.expression, "warm");
        assert_eq!(baseline.posture, "open");
        assert_eq!(baseline.motion_energy, 1.4);
        assert_eq!(baseline.attention, 0.0);
    }

    #[test]
    fn preserves_plain_replies_and_discards_invalid_cues() {
        assert_eq!(parse_chat_performance("plain reply").reply, "plain reply");
        assert!(parse_chat_performance("plain reply").plan.is_none());
        let invalid = parse_chat_performance(
            r#"{"reply":"Hi","performance":{"cues":[{"intent":"execute-code"}]}}"#,
        );
        assert!(invalid.plan.is_none());
        assert!(parse_performance_plan(
            r#"{"baseline":{"expression":"angry","posture":"attack"},"cues":[]}"#
        )
        .is_none());
    }
}
