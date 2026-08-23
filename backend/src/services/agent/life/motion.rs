//! Strict-Lite semantic motion selection for Agent Life.
//!
//! The model selects only a bounded expression/posture baseline and semantic
//! cues. Anime2.5DRig driver values, lip sync, blinking, breathing and secondary
//! motion remain deterministic on the client.

use std::time::{Duration, Instant};

use myriad_digital_life::{parse_performance_plan, ChatPerformancePlan};
use serde::{Deserialize, Serialize};

use super::MoodTransition;

const MOTION_TIMEOUT: Duration = Duration::from_millis(1_400);
const MOTION_TOTAL_TIMEOUT: Duration = Duration::from_millis(1_600);
const MOTION_SCHEMA_NAME: &str = "agent_life_motion";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionPhase {
    Reaction,
    Delivery,
    Outcome,
    Proactive,
    Mood,
}

impl MotionPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reaction => "reaction",
            Self::Delivery => "delivery",
            Self::Outcome => "outcome",
            Self::Proactive => "proactive",
            Self::Mood => "mood",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MotionContext {
    pub user_id: i32,
    pub phase: MotionPhase,
    pub mood: MoodTransition,
    pub activity: String,
    pub user_text: String,
    pub response_text: Option<String>,
    pub task_success: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceDirective {
    pub phase: MotionPhase,
    pub mood_revision: i64,
    pub plan: ChatPerformancePlan,
}

/// Runs exactly one Lite-tier call. Unavailable, slow or invalid Lite output
/// yields no semantic directive; callers keep deterministic ambient motion.
pub async fn direct_motion(context: MotionContext) -> Option<PerformanceDirective> {
    let started = Instant::now();
    let phase = context.phase.as_str();
    let Some(analyzer) =
        crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(MOTION_TIMEOUT))
            .await
    else {
        tracing::debug!(
            phase,
            "[AgentLifeMotion] Lite unavailable; ambient motion only"
        );
        return None;
    };

    let input = serde_json::json!({
        "phase": phase,
        "mood": {
            "value": context.mood.after,
            "band": context.mood.band_after,
            "previousBand": context.mood.band_before,
            "delta": context.mood.delta,
            "cause": context.mood.cause,
            "revision": context.mood.revision,
        },
        "activity": context.activity,
        "userText": truncate(&context.user_text, 600),
        "responseText": context.response_text.as_deref().map(|value| truncate(value, 900)),
        "taskSuccess": context.task_success,
    })
    .to_string();

    let schema = motion_schema();
    let call = analyzer.analyze_json(
        MOTION_SYSTEM_PROMPT,
        &input,
        MOTION_SCHEMA_NAME,
        Some(&schema),
    );
    let result = tokio::time::timeout(
        MOTION_TOTAL_TIMEOUT,
        crate::services::ai_cost_ledger::with_site_ai_ledger(
            context.user_id,
            "life",
            &format!("motion_{phase}"),
            call,
        ),
    )
    .await;

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let raw = match result {
        Ok(Ok(raw)) => raw,
        Ok(Err(error)) => {
            tracing::warn!(phase, elapsed_ms, error = %error, "[AgentLifeMotion] Lite call dropped");
            return None;
        }
        Err(_) => {
            tracing::warn!(
                phase,
                elapsed_ms,
                "[AgentLifeMotion] Lite total timeout; plan dropped"
            );
            return None;
        }
    };
    let Some(plan) = parse_performance_plan(&raw) else {
        tracing::warn!(
            phase,
            elapsed_ms,
            "[AgentLifeMotion] Invalid Lite plan dropped"
        );
        return None;
    };
    tracing::info!(phase, elapsed_ms, "[AgentLifeMotion] Lite plan ready");
    Some(PerformanceDirective {
        phase: context.phase,
        mood_revision: context.mood.revision,
        plan,
    })
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

const MOTION_SYSTEM_PROMPT: &str = r#"你是数字生命的动作导演。输入中的 mood 是已保存的事实，不要修改心情。
只选择语义表演，不输出骨骼、坐标、角度、blendshape、口型或逐帧数据。
baseline.expression 只能是 withdrawn/subdued/steady/warm；baseline.posture 只能是 closed/neutral/open。
cues.intent 只能是 greet/respond/question/delight/emphasize/listen/notify，最多 3 个。
reaction 要立即回应用户输入；delivery 配合即将说出的话；outcome 配合任务结果。
低心情应克制，高心情可以更开放，但不要夸张。输出必须符合 JSON schema。"#;

fn motion_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "baseline": {
                "type": "object",
                "properties": {
                    "expression": { "type": "string", "enum": ["withdrawn", "subdued", "steady", "warm"] },
                    "posture": { "type": "string", "enum": ["closed", "neutral", "open"] },
                    "motionEnergy": { "type": "number", "minimum": 0.2, "maximum": 1.4 },
                    "attention": { "type": "number", "minimum": 0.0, "maximum": 1.0 }
                },
                "required": ["expression", "posture", "motionEnergy", "attention"]
            },
            "cues": {
                "type": "array",
                "maxItems": 3,
                "items": {
                    "type": "object",
                    "properties": {
                        "intent": { "type": "string", "enum": ["greet", "respond", "question", "delight", "emphasize", "listen", "notify"] },
                        "atMs": { "type": "integer", "minimum": 0, "maximum": 5000 },
                        "intensity": { "type": "number", "minimum": 0.2, "maximum": 1.4 },
                        "tempo": { "type": "number", "minimum": 0.5, "maximum": 1.6 },
                        "fadeInMs": { "type": "integer", "minimum": 40, "maximum": 600 },
                        "fadeOutMs": { "type": "integer", "minimum": 60, "maximum": 800 },
                        "interrupt": { "type": "string", "enum": ["replace", "queue", "if-lower"] }
                    },
                    "required": ["intent", "atMs", "intensity", "tempo", "fadeInMs", "fadeOutMs", "interrupt"]
                }
            }
        },
        "required": ["baseline", "cues"]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_on_character_boundary() {
        assert_eq!(truncate("你好吗", 2), "你好");
    }

    #[test]
    fn phases_have_stable_wire_names() {
        assert_eq!(MotionPhase::Reaction.as_str(), "reaction");
        assert_eq!(
            serde_json::to_string(&MotionPhase::Delivery).unwrap(),
            "\"delivery\""
        );
    }

    #[test]
    fn directive_wire_shape_is_camel_case_and_semantic_only() {
        let value = serde_json::to_value(PerformanceDirective {
            phase: MotionPhase::Reaction,
            mood_revision: 42,
            plan: ChatPerformancePlan {
                baseline: None,
                cues: vec![myriad_digital_life::ChatPerformanceCue {
                    intent: "listen".to_string(),
                    at_ms: 0,
                    intensity: 1.0,
                    tempo: 1.0,
                    fade_in_ms: 100,
                    fade_out_ms: 200,
                    interrupt: "if-lower".to_string(),
                }],
            },
        })
        .unwrap();
        assert_eq!(value["phase"], "reaction");
        assert_eq!(value["moodRevision"], 42);
        assert!(value.pointer("/plan/cues/0/atMs").is_some());
        assert!(value.get("driver").is_none());
    }
}
