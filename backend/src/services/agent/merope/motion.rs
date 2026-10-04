//! Strict-Lite semantic motion selection for Merope.
//!
//! The model selects a bounded expression/posture baseline, semantic cues, and
//! grounded `phrases`. Anime2.5DRig driver values, lip sync, blinking, breathing
//! and secondary motion remain deterministic on the client.

use std::time::{Duration, Instant};

use myriad_merope::{
    ChatPerformanceBaseline, ChatPerformanceCue, ChatPerformancePlan,
    PERFORMANCE_BASELINE_EXPRESSIONS, PERFORMANCE_CUE_INTENTS, PERFORMANCE_INTERRUPT_MODES,
    PERFORMANCE_PHRASE_INTENTS, PERFORMANCE_POSTURES, RIG_STATE_MOTION_STYLES, RigStateSummary,
    SpeechPhrase, cue_is_playable, cue_survives_state, grounded_speech_phrases,
    parse_performance_plan, plan_is_empty, refine_performance_plan, round_motion_style,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::entities::agent_persona;

use super::MoodTransition;
use super::motion_local::local_performance_plan;
use super::store::get_persona;

mod prompt;
mod requests;

use prompt::*;
#[cfg(test)]
pub(in crate::services::agent) use prompt::{semantic_contract, semantic_valid};
pub use requests::*;

/// MOTION_TIMEOUT 9s; MOTION_TOTAL_TIMEOUT 10s.
/// Streaming publishes `local_directive` first; Lite is a refinement.
const MOTION_TIMEOUT: Duration = Duration::from_secs(9);
const MOTION_TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
const MOTION_SCHEMA_NAME: &str = "merope_motion";

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

    pub fn activity(self) -> &'static str {
        match self {
            Self::Reaction => "thinking",
            Self::Delivery | Self::Outcome | Self::Proactive => "talking",
            Self::Mood => "idle",
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
    /// Recently issued intentions, not proof that the body has played them.
    pub previous_phrases: Vec<SpeechPhrase>,
    pub task_success: Option<bool>,
    pub rig_state: Option<RigStateSummary>,
    /// Resolved once per Chat/Work round. Client style is only a fallback.
    pub motion_style: String,
}

#[derive(Debug, Clone, PartialEq)]
enum MotionDecision {
    Continue,
    Perform(ChatPerformancePlan),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceDirective {
    pub phase: MotionPhase,
    pub mood_revision: i64,
    pub motion_style: String,
    pub plan: ChatPerformancePlan,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub phrases: Vec<SpeechPhrase>,
}

/// Runs exactly one Lite-tier call, falling back to the deterministic plan for
/// callers (such as proactive speech) that have no live floor publisher.
pub async fn direct_motion(context: MotionContext) -> Option<PerformanceDirective> {
    direct_motion_inner(context, true).await
}

/// Refines a floor that has already been published. A timeout, invalid answer
/// or `continue` produces nothing so the same local beat is never replayed.
pub async fn refine_motion(context: MotionContext) -> Option<PerformanceDirective> {
    direct_motion_inner(context, false).await
}

async fn direct_motion_inner(
    context: MotionContext,
    fallback_to_local: bool,
) -> Option<PerformanceDirective> {
    let started = Instant::now();
    let phase = context.phase.as_str();
    let rig_state = apply_round_motion_style(context.rig_state.clone(), &context.motion_style);
    if face_is_hidden(rig_state.as_ref()) {
        tracing::debug!(phase, "[MeropeMotion] Face hidden; ambient motion only");
        return None;
    }
    let analyzer =
        crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(MOTION_TIMEOUT))
            .await;
    let result = if let Some(analyzer) = analyzer {
        let persona_row = match crate::services::process_db::database() {
            Ok(db) => get_persona(&db).await.ok().flatten(),
            Err(_) => None,
        };
        let input = motion_input(&context, persona_row.as_ref()).to_string();

        // Offer only what this face can actually play. Constrained decoding
        // then cannot spend the round's 0–2 cues on something the filter below
        // would delete.
        let offered = offered_cue_intents(rig_state.as_ref());
        let schema = motion_schema_for_state(&offered, rig_state.as_ref());
        let system_prompt = motion_system_prompt(&offered);
        let call = analyzer.analyze_json(&system_prompt, &input, MOTION_SCHEMA_NAME, Some(&schema));
        Some(
            tokio::time::timeout(
                MOTION_TOTAL_TIMEOUT,
                crate::services::ai_cost_ledger::with_site_ai_ledger(
                    context.user_id,
                    "merope",
                    &format!("motion_{phase}"),
                    call,
                ),
            )
            .await,
        )
    } else {
        tracing::debug!(
            phase,
            "[MeropeMotion] Lite unavailable; requested cues may still play"
        );
        None
    };

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let mut phrases = Vec::new();
    let lite_plan = match result {
        None => None,
        Some(Ok(Ok(raw))) => match parse_motion_decision(&raw) {
            Some(MotionDecision::Continue) => {
                tracing::debug!(
                    phase,
                    elapsed_ms,
                    "[MeropeMotion] Lite continued current acting"
                );
                // Deliberate continuation is not a failed model call. In
                // particular, direct/proactive callers must not replay a local
                // fallback baseline when the director asked to leave it alone.
                return None;
            }
            Some(MotionDecision::Perform(parsed)) => {
                if let Ok(value) = serde_json::from_str::<Value>(strip_motion_json(&raw)) {
                    phrases = grounded_speech_phrases(
                        &value["phrases"],
                        context.response_text.as_deref(),
                    );
                }
                Some(parsed)
            }
            None => {
                tracing::warn!(
                    phase,
                    elapsed_ms,
                    "[MeropeMotion] Invalid Lite plan dropped"
                );
                None
            }
        },
        Some(Ok(Err(error))) => {
            tracing::warn!(phase, elapsed_ms, error = %error, "[MeropeMotion] Lite call dropped");
            None
        }
        Some(Err(_)) => {
            tracing::warn!(
                phase,
                elapsed_ms,
                "[MeropeMotion] Lite total timeout; plan dropped"
            );
            None
        }
    };
    let refined_by_lite = lite_plan.is_some();
    let mut plan = if let Some(plan) = lite_plan {
        plan
    } else if fallback_to_local {
        local_performance_plan(
            context.phase,
            &context.mood,
            context.task_success,
            context.response_text.as_deref(),
            &context.motion_style,
            Some(&context.user_text),
        )
    } else {
        return None;
    };
    if let Some(state) = rig_state.as_ref() {
        plan = refine_performance_plan(plan, state);
    }
    // A named ask is not a command; overlay only after this turn's reply.
    plan = apply_user_requested_cue(
        plan,
        context.phase,
        &context.user_text,
        context.response_text.as_deref(),
        rig_state.as_ref(),
    );
    if plan_is_empty(&plan) && phrases.is_empty() {
        tracing::debug!(
            phase,
            elapsed_ms,
            "[MeropeMotion] Lite plan had no capable cues"
        );
        return None;
    }
    tracing::info!(
        phase,
        elapsed_ms,
        source = if refined_by_lite { "lite" } else { "local" },
        requested = play_along_requested_cue(
            context.phase,
            &context.user_text,
            context.response_text.as_deref(),
        )
        .is_some(),
        "[MeropeMotion] plan ready"
    );
    Some(PerformanceDirective {
        phase: context.phase,
        mood_revision: context.mood.revision,
        motion_style: context.motion_style,
        plan,
        phrases,
    })
}

/// The deterministic floor as a directive, with no network call and no await.
///
/// Chat plays this the moment the round starts so the character reacts before
/// it speaks. Non-streaming attaches it on the body; streaming Chat leaves
/// `performance: None` and Lite replaces via PlaybackDirection / `PerformancePlan`.
pub fn local_directive(context: &MotionContext) -> Option<PerformanceDirective> {
    let rig_state = apply_round_motion_style(context.rig_state.clone(), &context.motion_style);
    if face_is_hidden(rig_state.as_ref()) {
        return None;
    }
    let mut plan = local_performance_plan(
        context.phase,
        &context.mood,
        context.task_success,
        context.response_text.as_deref(),
        &context.motion_style,
        Some(&context.user_text),
    );
    if let Some(state) = rig_state.as_ref() {
        plan = refine_performance_plan(plan, state);
    }
    plan = apply_user_requested_cue(
        plan,
        context.phase,
        &context.user_text,
        context.response_text.as_deref(),
        rig_state.as_ref(),
    );
    if plan_is_empty(&plan) {
        return None;
    }
    Some(PerformanceDirective {
        phase: context.phase,
        mood_revision: context.mood.revision,
        motion_style: context.motion_style.clone(),
        plan,
        phrases: Vec::new(),
    })
}

/// One persona read per Chat/Work round. Client style is only used when the
/// site persona cannot be loaded.
pub async fn resolve_round_motion_style(
    client: Option<&RigStateSummary>,
    mood: i32,
    arousal: i32,
) -> String {
    let client_style = client.map(|summary| summary.motion_style.as_str());
    if let Ok(db) = crate::services::process_db::database() {
        if let Ok(Some(persona)) = get_persona(&db).await {
            return round_motion_style(
                client_style,
                persona.persona_json.as_ref(),
                true,
                mood,
                arousal,
            );
        }
    }
    round_motion_style(client_style, None, false, mood, arousal)
}

fn apply_round_motion_style(
    summary: Option<RigStateSummary>,
    motion_style: &str,
) -> Option<RigStateSummary> {
    let mut summary = summary?;
    summary.motion_style = if RIG_STATE_MOTION_STYLES.contains(&motion_style) {
        motion_style.to_string()
    } else {
        summary.motion_style
    };
    Some(summary)
}

fn face_is_hidden(state: Option<&RigStateSummary>) -> bool {
    state.is_some_and(|summary| !summary.page_visible || !summary.face_visible)
}

fn parse_motion_decision(raw: &str) -> Option<MotionDecision> {
    let stripped = strip_motion_json(raw);
    let value: serde_json::Value = serde_json::from_str(stripped).ok()?;
    let object = value.as_object()?;
    match object.get("continue") {
        Some(flag) if !flag.is_boolean() => return None,
        Some(flag) if flag.as_bool() == Some(true) => {
            if motion_payload_present(object) {
                return None;
            }
            return Some(MotionDecision::Continue);
        }
        _ => {}
    }
    if let Some(plan) = parse_performance_plan(stripped) {
        return Some(MotionDecision::Perform(plan));
    }
    // A grounded phrase refinement does not need to replace the standing face.
    // Do not turn an invalid baseline/cue payload into a phrase-only success.
    let empty_plan = object.get("baseline").is_none_or(Value::is_null)
        && object
            .get("cues")
            .is_none_or(|value| value.as_array().is_some_and(Vec::is_empty));
    let valid_phrase = object
        .get("phrases")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().take(6).any(|item| {
                !grounded_speech_phrases(
                    &serde_json::json!([item]),
                    item.get("text").and_then(Value::as_str),
                )
                .is_empty()
            })
        });
    (empty_plan && valid_phrase).then(|| MotionDecision::Perform(ChatPerformancePlan::default()))
}

fn motion_payload_present(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    let has_baseline = object.get("baseline").is_some_and(|value| !value.is_null());
    let has_cues = object
        .get("cues")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|cues| !cues.is_empty());
    has_baseline
        || has_cues
        || object
            .get("phrases")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
}

fn strip_motion_json(raw: &str) -> &str {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|inner| inner.strip_suffix("```"))
        .unwrap_or(trimmed)
        .trim()
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
pub(super) mod live_probe {
    use super::*;

    pub fn contract(
        persona: &agent_persona::Model,
        rig: &RigStateSummary,
    ) -> (String, Value, Value) {
        let offered = offered_cue_intents(Some(rig));
        (
            motion_system_prompt(&offered),
            motion_schema(&offered),
            motion_persona_payload(Some(persona), "even"),
        )
    }

    pub fn valid(raw: &str, rig: &RigStateSummary) -> bool {
        let Ok(unfiltered) = serde_json::from_str::<ChatPerformancePlan>(raw) else {
            return false;
        };
        let Some(MotionDecision::Perform(plan)) = parse_motion_decision(raw) else {
            return false;
        };
        // A smoke test must not pass merely because production's safe parser
        // removed an invalid cue or clamped an out-of-range value.
        plan == unfiltered
            && plan.cues.len() <= 2
            && plan.baseline.is_some()
            && plan
                .cues
                .iter()
                .all(|cue| cue_survives_state(rig, &cue.intent))
    }

    #[test]
    fn smoke_rejects_cues_discarded_by_the_production_sanitizer() {
        let rig = myriad_merope::sanitize_rig_state(&serde_json::json!({})).unwrap();
        let valid = serde_json::json!({"baseline":{"expression":"steady","posture":"neutral","motionEnergy":1.0,"attention":0.5},"cues":[]});
        assert!(self::valid(&valid.to_string(), &rig));
        let mut invalid = valid;
        invalid["cues"] = serde_json::json!([{"intent":"not_a_cue","atMs":0,"intensity":1.0,"tempo":1.0,"fadeInMs":100,"fadeOutMs":200,"interrupt":"blend"}]);
        assert!(!self::valid(&invalid.to_string(), &rig));
    }
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;
