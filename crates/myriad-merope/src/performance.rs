use serde::{Deserialize, Serialize};

use crate::rig_contract::{
    PERFORMANCE_BASELINE_EXPRESSIONS, PERFORMANCE_CUE_INTENTS, PERFORMANCE_INTERRUPT_MODES,
    PERFORMANCE_POSTURES,
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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

/// Parses the Lite motion director's plan. Semantic acting is selected here and
/// nowhere else: a reply model never gets to author its own performance.
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
    if baseline.is_none() && cues.is_empty() {
        return None;
    }
    Some(ChatPerformancePlan { baseline, cues })
}

fn sanitize_baseline(baseline: RawBaseline) -> Option<ChatPerformanceBaseline> {
    if !PERFORMANCE_BASELINE_EXPRESSIONS.contains(&baseline.expression.as_str())
        || !PERFORMANCE_POSTURES.contains(&baseline.posture.as_str())
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
    if !PERFORMANCE_CUE_INTENTS.contains(&cue.intent.as_str())
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
        interrupt: if PERFORMANCE_INTERRUPT_MODES.contains(&cue.interrupt.as_str()) {
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
    fn lite_plan_parser_bounds_out_of_range_cues() {
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
    fn accepts_think_dizzy_and_cry_as_bounded_semantic_cues() {
        let plan = parse_performance_plan(
            r#"{"cues":[{"intent":"think","atMs":0,"intensity":0.9,"tempo":0.8,"fadeInMs":160,"fadeOutMs":320,"interrupt":"queue"},{"intent":"dizzy","atMs":120,"intensity":1.2,"tempo":0.8,"fadeInMs":180,"fadeOutMs":420,"interrupt":"if-lower"},{"intent":"cry","atMs":180,"intensity":1.4,"tempo":0.7,"fadeInMs":260,"fadeOutMs":500,"interrupt":"replace"}]}"#,
        )
        .unwrap();
        assert_eq!(plan.cues.len(), 3);
        assert_eq!(plan.cues[0].intent, "think");
        assert_eq!(plan.cues[1].intent, "dizzy");
        assert_eq!(plan.cues[1].at_ms, 120);
        assert_eq!(plan.cues[2].intent, "cry");
        assert_eq!(plan.cues[2].fade_out_ms, 500);
    }

    #[test]
    fn accepts_stylized_semantic_cues() {
        let plan = parse_performance_plan(
            r#"{"cues":[{"intent":"angry","intensity":1.1},{"intent":"speechless","intensity":0.8},{"intent":"silly","intensity":1.0}]}"#,
        )
        .unwrap();
        assert_eq!(plan.cues.len(), 3);
        assert_eq!(plan.cues[0].intent, "angry");
        assert_eq!(plan.cues[1].intent, "speechless");
        assert_eq!(plan.cues[2].intent, "silly");
    }

    #[test]
    fn accepts_lovestruck_as_a_semantic_cue() {
        let plan = parse_performance_plan(r#"{"cues":[{"intent":"lovestruck"}]}"#).unwrap();
        assert_eq!(plan.cues[0].intent, "lovestruck");
    }

    #[test]
    fn discards_plans_whose_baseline_is_out_of_vocabulary() {
        assert!(parse_performance_plan(
            r#"{"baseline":{"expression":"angry","posture":"attack"},"cues":[]}"#,
        )
        .is_none());
    }

    #[test]
    fn empty_object_is_not_a_plan() {
        assert!(parse_performance_plan("{}").is_none());
        assert!(parse_performance_plan(r#"{"cues":[]}"#).is_none());
    }

    #[test]
    fn continue_is_not_a_public_plan() {
        assert!(parse_performance_plan(r#"{"continue":true}"#).is_none());
    }

    #[test]
    fn mouth_open_is_not_a_cue_intent() {
        assert!(parse_performance_plan(r#"{"cues":[{"intent":"mouth-open"}]}"#).is_none());
    }
}
