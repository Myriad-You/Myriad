//! Her acting, directed by one director: the judgment model answering typed
//! questions about the moment (`questions`). Its answers become a bounded
//! expression/posture baseline, cues, grounded `phrases` and a `score`; the
//! floor shows her state until it lands or when it has nothing. Anime2.5DRig
//! driver values, lip sync, blinking, breathing and secondary motion remain
//! deterministic on the client.
//!
//! The judgments (which questions, how the answers are read and land on her
//! body, the floor, her standing acting) are the shared core's
//! `myriad_merope::motion`; asking the model, its time, the persona read and
//! the cost ledger are here.

use std::time::{Duration, Instant};

use myriad_merope::motion::{Moment, Outcome};
pub use myriad_merope::motion::{MotionPhase, PerformanceDirective};
use myriad_merope::{RigStateSummary, SpeechPhrase, round_motion_style};

use super::MoodTransition;
use super::store::get_persona;

mod questions;

#[cfg(test)]
pub(in crate::services::agent) use myriad_merope::motion::semantic_valid;
#[cfg(test)]
pub(in crate::services::agent) use questions::semantic_contract;

/// MOTION_TIMEOUT 9s; MOTION_TOTAL_TIMEOUT 10s.
/// Streaming publishes `local_directive` first; the director refines it.
const MOTION_TIMEOUT: Duration = Duration::from_secs(9);
const MOTION_TOTAL_TIMEOUT: Duration = Duration::from_secs(10);
/// Standing acting waits for no reply and is kept for the whole activity, so
/// a chat model may take its time over every control (5–12 s measured).
const STANDING_TIMEOUT: Duration = Duration::from_secs(28);
const STANDING_TOTAL_TIMEOUT: Duration = Duration::from_secs(30);

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

impl MotionContext {
    /// The moment as the core's director judges it (everything but whose
    /// round it is).
    pub(in crate::services::agent) fn moment(&self) -> Moment {
        Moment {
            phase: self.phase,
            mood: self.mood.clone(),
            activity: self.activity.clone(),
            user_text: self.user_text.clone(),
            response_text: self.response_text.clone(),
            previous_phrases: self.previous_phrases.clone(),
            task_success: self.task_success,
            rig_state: self.rig_state.clone(),
            motion_style: self.motion_style.clone(),
        }
    }
}

/// The model that directs her acting: the judgment model, quick and thinking
/// little, since acting is a small typed judgment that must land in time;
/// Lite's own model when no judgment model is set.
pub(in crate::services::agent) async fn director_analyzer(
    timeout: Duration,
) -> Option<crate::services::analyzer::AiAnalyzer> {
    match crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(timeout)).await {
        Some(judge) => Some(judge),
        None => {
            crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(timeout)).await
        }
    }
}

/// The judgment model's source when it answers decisions (TypeSafe Jev)
/// rather than writing a plan.
pub(in crate::services::agent) async fn decisions_source() -> Option<crate::config::ResolvedAiConfig>
{
    crate::services::ai::learn_judge_model_kind().await;
    crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .resolve_decisions_ai_config()
}

/// What the judgment model makes of a moment by answering the questions, as
/// the director's plan: on the decisions endpoint when `source` is given,
/// otherwise `analyzer` answers as structured output.
#[cfg(test)]
pub(in crate::services::agent) async fn answered_plan(
    context: &MotionContext,
    persona: Option<&str>,
    source: Option<&crate::config::ResolvedAiConfig>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
) -> anyhow::Result<String> {
    let moment = context.moment();
    let state = context.rig_state.as_ref();
    let budget = if context.phase == MotionPhase::Presence {
        STANDING_TIMEOUT
    } else {
        MOTION_TIMEOUT
    };
    match source {
        Some(source) => questions::direct(&moment, state, persona, source, budget).await,
        None => questions::direct_by_chat(&moment, state, persona, analyzer, budget).await,
    }
}

/// Asks the director once, falling back to the floor for callers (such as
/// proactive speech) that have no live floor publisher.
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
    let moment = context.moment();
    let rig_state = moment.round_rig_state();
    if moment.face_hidden() {
        tracing::debug!(phase, "[MeropeMotion] Face hidden; ambient motion only");
        return None;
    }
    // Her only director: the judgment model, answering typed questions about
    // the moment (a decisions model on its own endpoint, a chat model as
    // structured output); her standing acting too, as a score looped on time.
    // When it has no answer, the floor already playing stands.
    let persona_row = match crate::services::process_db::database() {
        Ok(db) => get_persona(&db).await.ok().flatten(),
        Err(_) => None,
    };
    let persona = persona_row
        .as_ref()
        .map(|row| row.personality.trim().to_owned());
    let persona = persona.as_deref().filter(|p| !p.is_empty());
    let (budget, total) = if context.phase == MotionPhase::Presence {
        (STANDING_TIMEOUT, STANDING_TOTAL_TIMEOUT)
    } else {
        (MOTION_TIMEOUT, MOTION_TOTAL_TIMEOUT)
    };
    let call = async {
        if let Some(source) = decisions_source().await {
            questions::direct(&moment, rig_state.as_ref(), persona, &source, budget).await
        } else if let Some(analyzer) = director_analyzer(budget).await {
            questions::direct_by_chat(&moment, rig_state.as_ref(), persona, &analyzer, budget).await
        } else {
            Err(anyhow::anyhow!("no judgment model"))
        }
    };
    let result = tokio::time::timeout(
        total,
        crate::services::ai_cost_ledger::with_site_ai_ledger(
            context.user_id,
            "merope",
            &format!("motion_{phase}"),
            call,
        ),
    )
    .await;

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let answer = match result {
        Ok(Ok(raw)) => Some(raw),
        Ok(Err(error)) => {
            tracing::warn!(phase, elapsed_ms, error = %error, "[MeropeMotion] Director had no answer");
            None
        }
        Err(_) => {
            tracing::warn!(phase, elapsed_ms, "[MeropeMotion] Director timed out");
            None
        }
    };
    let direction = myriad_merope::motion::direction(&moment, answer.as_deref(), fallback_to_local);
    if direction.dropped {
        tracing::warn!(phase, elapsed_ms, "[MeropeMotion] Invalid plan dropped");
    }
    match direction.outcome {
        Outcome::Continued => {
            tracing::debug!(phase, elapsed_ms, "[MeropeMotion] Director continued");
        }
        Outcome::Unplayable => {
            tracing::debug!(phase, elapsed_ms, "[MeropeMotion] Plan had no capable cues");
        }
        Outcome::Director | Outcome::Floor => {
            tracing::info!(
                phase,
                elapsed_ms,
                source = if direction.outcome == Outcome::Director {
                    "director"
                } else {
                    "floor"
                },
                "[MeropeMotion] plan ready"
            );
        }
        Outcome::Hidden | Outcome::Nothing => {}
    }
    direction.directive
}

/// Her standing acting when the director cannot be reached (offline): shown
/// the `way` what she is doing shows by its kind, with no network call.
pub fn standing_floor(context: &MotionContext, way: &str) -> Option<PerformanceDirective> {
    myriad_merope::motion::standing_floor(&context.moment(), way)
}

/// The deterministic floor as a directive, with no network call and no await.
///
/// Chat plays this the moment the round starts so the character reacts before
/// it speaks. Non-streaming attaches it on the body; streaming Chat leaves
/// `performance: None` and the director replaces it via PlaybackDirection /
/// `PerformancePlan`.
pub fn local_directive(context: &MotionContext) -> Option<PerformanceDirective> {
    myriad_merope::motion::local_directive(&context.moment())
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

/// A candidate judgment model tried on the director's questions (live acceptance).
#[cfg(test)]
pub(super) mod live_probe {
    use super::*;
    use crate::models::entities::agent_persona;
    use myriad_merope::motion::{
        MotionDecision, parse_motion_decision, questions, strip_motion_json,
    };
    use serde_json::Value;

    fn moment(rig: &RigStateSummary) -> Moment {
        MotionContext {
            user_id: 0,
            phase: MotionPhase::Delivery,
            mood: MoodTransition {
                before: 65.0,
                after: 65.0,
                arousal_before: 48.0,
                arousal_after: 48.0,
                band_before: "calm".into(),
                band_after: "calm".into(),
                delta: 0.0,
                cause: "user_utterance".into(),
                revision: 1,
            },
            activity: "talking".into(),
            user_text: String::new(),
            response_text: None,
            previous_phrases: Vec::new(),
            task_success: None,
            rig_state: Some(rig.clone()),
            motion_style: "even".into(),
        }
        .moment()
    }

    /// The system prompt, schema and persona a candidate answers with.
    pub fn contract(
        persona: &agent_persona::Model,
        rig: &RigStateSummary,
    ) -> (String, Value, Value) {
        let asked = questions::questions(&moment(rig), Some(rig));
        (
            questions::chat_system_prompt(&asked),
            questions::chat_schema(&asked),
            serde_json::json!(persona.personality),
        )
    }

    /// Whether a candidate's answers become a plan production would play.
    pub fn valid(raw: &str, rig: &RigStateSummary) -> bool {
        let Ok(answers) = serde_json::from_str::<Value>(strip_motion_json(raw)) else {
            return false;
        };
        let context = moment(rig);
        let asked = questions::questions(&context, Some(rig));
        let answered = asked
            .as_object()
            .is_some_and(|asked| asked.keys().all(|key| answers[key].is_object()));
        answered
            && matches!(
                parse_motion_decision(
                    &questions::plan_from_answers(&answers, &context, Some(rig)).to_string()
                ),
                Some(MotionDecision::Perform(_))
            )
    }

    #[test]
    fn a_candidate_must_answer_every_question() {
        let rig = myriad_merope::sanitize_rig_state(&serde_json::json!({})).unwrap();
        let answers = serde_json::json!({
            "feeling": {"choice": "calm", "confidence": 0.8},
            "own": {"noul": 0.9},
            "posture": {"choice": "neutral", "confidence": 0.7},
        });
        let asked = questions::questions(&moment(&rig), Some(&rig));
        let mut full = answers.clone();
        for key in asked.as_object().unwrap().keys() {
            if full[key].is_null() {
                full[key] = serde_json::json!({"choice": "none", "confidence": 0.1, "score": 0.0});
            }
        }
        assert!(self::valid(&full.to_string(), &rig));
        if asked.as_object().unwrap().len() > 3 {
            assert!(!self::valid(&answers.to_string(), &rig));
        }
    }
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;
