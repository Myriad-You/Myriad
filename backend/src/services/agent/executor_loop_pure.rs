//! Pure helpers for executor_loop_pure.


use crate::services::agent::types::{FailureStrategy, StepResult, TaskStatus};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Hard cap on steps in a single execute run (incl. dynamic).
pub const MAX_TOTAL_STEPS: usize = 15;
/// Hard cap on steps during resume_with_answer continuation.
pub const MAX_RESUME_STEPS: usize = 50;
/// Cap on AI dynamic-analysis generations per task context.
pub const MAX_DYNAMIC_ANALYSIS_STEPS: usize = 15;
/// Cap on user questions per task (answered count).
pub const MAX_QUESTIONS_PER_TASK: usize = 3;
/// Default skill sub-step timeout (ms).
pub const SKILL_SUBSTEP_TIMEOUT_MS: u64 = 60_000;

/// Skill orchestrator steps (`skill:…`) hide frontend step events.
pub fn is_skill_planning_step(capability_id: &str) -> bool {
    capability_id.starts_with("skill:")
}

/// Whether execute should break for the total step ceiling.
pub fn exceeded_total_step_limit(executed: usize) -> bool {
    executed >= MAX_TOTAL_STEPS
}

/// Whether resume should break for the resume step ceiling.
pub fn exceeded_resume_step_limit(executed: usize) -> bool {
    executed > MAX_RESUME_STEPS
}

/// Aggregate terminal status after a run finishes (not mid-wait).
///
/// - cancelled wins
/// - all recorded steps failed → Failed (joined errors)
/// - otherwise Completed (partial success included)
#[derive(Debug, Clone, PartialEq)]
pub struct TerminalRunOutcome {
    pub status: TaskStatus,
    pub error: Option<String>,
}

pub fn resolve_terminal_run_outcome(
    cancelled: bool,
    cancelled_message: &str,
    step_results: &HashMap<String, StepResult>,
) -> TerminalRunOutcome {
    if cancelled {
        return TerminalRunOutcome {
            status: TaskStatus::Cancelled,
            error: Some(cancelled_message.to_string()),
        };
    }

    let total = step_results.len();
    let failed: Vec<&StepResult> = step_results.values().filter(|r| !r.success).collect();

    if total > 0 && failed.len() == total {
        let errors: Vec<String> = failed.iter().filter_map(|r| r.error.clone()).collect();
        TerminalRunOutcome {
            status: TaskStatus::Failed,
            error: Some(errors.join("; ")),
        }
    } else {
        TerminalRunOutcome {
            status: TaskStatus::Completed,
            error: None,
        }
    }
}

/// Why dynamic analysis (post-step AI question) should be skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicAnalysisSkip {
    MaxDynamicSteps,
    MaxQuestions,
    AiProcessingCapability,
    DownstreamAiWillProcess,
}

/// Gate for `analyze_and_generate_dynamic_steps` before any AI call.
pub fn dynamic_analysis_skip_reason(
    capability_id: &str,
    dynamic_steps_generated: usize,
    questions_asked: usize,
    has_downstream_ai: bool,
) -> Option<DynamicAnalysisSkip> {
    if dynamic_steps_generated >= MAX_DYNAMIC_ANALYSIS_STEPS {
        return Some(DynamicAnalysisSkip::MaxDynamicSteps);
    }
    if questions_asked >= MAX_QUESTIONS_PER_TASK {
        return Some(DynamicAnalysisSkip::MaxQuestions);
    }
    if is_ai_processing_capability(capability_id) {
        return Some(DynamicAnalysisSkip::AiProcessingCapability);
    }
    if has_downstream_ai {
        return Some(DynamicAnalysisSkip::DownstreamAiWillProcess);
    }
    None
}

/// Capabilities whose output is already AI reasoning — no second-pass question.
pub fn is_ai_processing_capability(capability_id: &str) -> bool {
    capability_id.starts_with("ai.")
        || capability_id.starts_with("compare.")
        || capability_id == "prompt.generate"
        || capability_id == "translate.text"
        || capability_id == "code.explain"
}

/// Whether a recipe step is an AI consumer of `source_step_id` output.
pub fn is_downstream_ai_step(capability_id: &str) -> bool {
    capability_id.starts_with("ai.") || capability_id == "prompt.generate"
}

/// Recipe has a later AI step depending on `source_step_id`.
pub fn has_downstream_ai_for_step<'a, I>(source_step_id: &str, steps: I) -> bool
where
    I: IntoIterator<Item = &'a (String, String, Vec<String>)>,
{
    // (id, capability_id, depends_on) — avoid pulling full RecipeStep into tests only
    steps.into_iter().any(|(id, cap, deps)| {
        id != source_step_id
            && deps.iter().any(|d| d == source_step_id)
            && is_downstream_ai_step(cap)
    })
}

/// Convenience over recipe step slices.
pub fn has_downstream_ai_for_recipe_steps(
    source_step_id: &str,
    steps: &[crate::services::agent::types::RecipeStep],
) -> bool {
    steps.iter().any(|s| {
        s.id != source_step_id
            && s.depends_on.iter().any(|d| d == source_step_id)
            && is_downstream_ai_step(&s.capability_id)
    })
}

/// First non-AI skill sub-step aborts on failure; others skip.
pub fn skill_substep_on_failure(index: usize, capability_id: &str) -> FailureStrategy {
    if index == 0 && !capability_id.starts_with("ai.") {
        FailureStrategy::Abort
    } else {
        FailureStrategy::Skip
    }
}

/// Whether a capability is allowed under skill gating (empty gate = open).
pub fn skill_capability_allowed_by_gating(
    capability_id: &str,
    gated_capabilities: &[String],
) -> bool {
    if gated_capabilities.is_empty() {
        return true;
    }
    if capability_id.starts_with("ai.")
        || capability_id.starts_with("skill:")
        || capability_id.starts_with("mcp.")
    {
        return true;
    }
    gated_capabilities.iter().any(|c| c == capability_id)
}

/// Rewrite a `xxxFrom` reference using skill AI-id → real step-id map.
///
/// Returns the new string value when rewritten; `None` if not a From key or
/// no mapping applied (caller keeps original).
pub fn rewrite_skill_from_param(
    param_key: &str,
    value: &Value,
    id_map: &HashMap<String, String>,
) -> Option<Value> {
    if !param_key.ends_with("From") {
        return None;
    }
    let ref_str = value.as_str()?;
    let parts: Vec<&str> = ref_str.splitn(2, '.').collect();
    let actual_id = id_map.get(parts[0])?;
    let new_ref = if parts.len() > 1 {
        format!("{}.{}", actual_id, parts[1])
    } else {
        actual_id.clone()
    };
    Some(Value::String(new_ref))
}

/// Resolve depends_on entries for a skill sub-step.
///
/// - skipped AI ids → dropped (sets `broken` if any)
/// - parent step ids → dropped (already done; data via From)
/// - mapped AI ids → real ids
/// - unknown → dropped
pub fn resolve_skill_depends_on(
    deps: &[String],
    id_map: &HashMap<String, String>,
    skipped_ai_ids: &HashSet<String>,
    parent_step_ids: &HashSet<String>,
) -> (Vec<String>, bool) {
    let mut broken = false;
    let mut out = Vec::new();
    for dep in deps {
        if skipped_ai_ids.contains(dep) {
            broken = true;
            continue;
        }
        if parent_step_ids.contains(dep) {
            continue;
        }
        if let Some(actual) = id_map.get(dep) {
            out.push(actual.clone());
        }
        // unknown: drop silently (adapter may log)
    }
    (out, broken)
}

/// Planned skill placeholder output consumed by resolve_path_reference.
pub fn skill_planned_output(skill_id: &str, substep_ids: &[String]) -> Value {
    json!({
        "skill": skill_id,
        "status": "planned",
        "dynamic_steps_generated": substep_ids.len(),
        "__substep_ids": substep_ids,
    })
}

/// Extract non-empty error string from a step output object, if any.
pub fn step_output_error_message(output: &Value) -> Option<&str> {
    output
        .get("error")
        .and_then(|e| e.as_str())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::types::StepResult;

    fn sr(ok: bool, err: Option<&str>) -> StepResult {
        StepResult {
            step_id: "s".into(),
            success: ok,
            output: None,
            error: err.map(String::from),
            duration_ms: 1,
            retry_count: 0,
        }
    }

    #[test]
    fn terminal_cancelled_all_fail_complete() {
        let mut results = HashMap::new();
        results.insert("a".into(), sr(false, Some("e1")));
        results.insert("b".into(), sr(false, Some("e2")));

        let c = resolve_terminal_run_outcome(true, "cancelled", &results);
        assert_eq!(c.status, TaskStatus::Cancelled);
        assert_eq!(c.error.as_deref(), Some("cancelled"));

        let f = resolve_terminal_run_outcome(false, "x", &results);
        assert_eq!(f.status, TaskStatus::Failed);
        assert!(f.error.as_ref().unwrap().contains("e1"));
        assert!(f.error.as_ref().unwrap().contains("e2"));

        results.insert("c".into(), sr(true, None));
        let ok = resolve_terminal_run_outcome(false, "x", &results);
        assert_eq!(ok.status, TaskStatus::Completed);
        assert!(ok.error.is_none());

        let empty = resolve_terminal_run_outcome(false, "x", &HashMap::new());
        assert_eq!(empty.status, TaskStatus::Completed);
    }

    #[test]
    fn step_limits_and_skill_planning() {
        assert!(is_skill_planning_step("skill:foo"));
        assert!(!is_skill_planning_step("ai.chat"));
        assert!(!exceeded_total_step_limit(14));
        assert!(exceeded_total_step_limit(15));
        assert!(!exceeded_resume_step_limit(50));
        assert!(exceeded_resume_step_limit(51));
    }

    #[test]
    fn dynamic_analysis_gates() {
        assert_eq!(
            dynamic_analysis_skip_reason("web.search", 15, 0, false),
            Some(DynamicAnalysisSkip::MaxDynamicSteps)
        );
        assert_eq!(
            dynamic_analysis_skip_reason("web.search", 0, 3, false),
            Some(DynamicAnalysisSkip::MaxQuestions)
        );
        assert_eq!(
            dynamic_analysis_skip_reason("ai.analyze", 0, 0, false),
            Some(DynamicAnalysisSkip::AiProcessingCapability)
        );
        assert_eq!(
            dynamic_analysis_skip_reason("prompt.generate", 0, 0, false),
            Some(DynamicAnalysisSkip::AiProcessingCapability)
        );
        assert_eq!(
            dynamic_analysis_skip_reason("web.search", 0, 0, true),
            Some(DynamicAnalysisSkip::DownstreamAiWillProcess)
        );
        assert!(dynamic_analysis_skip_reason("web.search", 0, 0, false).is_none());
    }

    #[test]
    fn downstream_ai_and_gating() {
        let steps = vec![
            ("search".into(), "web.search".into(), vec![]),
            (
                "analyze".into(),
                "ai.analyze".into(),
                vec!["search".into()],
            ),
        ];
        assert!(has_downstream_ai_for_step("search", &steps));
        assert!(!has_downstream_ai_for_step("analyze", &steps));

        assert!(skill_capability_allowed_by_gating("ai.chat", &[]));
        assert!(skill_capability_allowed_by_gating(
            "ai.chat",
            &["web.search".into()]
        ));
        assert!(!skill_capability_allowed_by_gating(
            "music.play",
            &["web.search".into()]
        ));
        assert!(skill_capability_allowed_by_gating(
            "web.search",
            &["web.search".into()]
        ));
    }

    #[test]
    fn skill_depends_from_and_failure_policy() {
        assert_eq!(
            skill_substep_on_failure(0, "http.fetch"),
            FailureStrategy::Abort
        );
        assert_eq!(
            skill_substep_on_failure(0, "ai.chat"),
            FailureStrategy::Skip
        );
        assert_eq!(
            skill_substep_on_failure(1, "http.fetch"),
            FailureStrategy::Skip
        );

        let mut id_map = HashMap::new();
        id_map.insert("gen_a".into(), "parent_skill_step_0".into());
        let mut skipped = HashSet::new();
        skipped.insert("gen_skip".into());
        let mut parents = HashSet::new();
        parents.insert("parent".into());

        let (deps, broken) = resolve_skill_depends_on(
            &["gen_a".into(), "gen_skip".into(), "parent".into(), "missing".into()],
            &id_map,
            &skipped,
            &parents,
        );
        assert!(broken);
        assert_eq!(deps, vec!["parent_skill_step_0".to_string()]);

        let rewritten = rewrite_skill_from_param(
            "dataFrom",
            &json!("gen_a.results"),
            &id_map,
        );
        assert_eq!(
            rewritten,
            Some(json!("parent_skill_step_0.results"))
        );
        assert!(rewrite_skill_from_param("data", &json!("x"), &id_map).is_none());
    }

    #[test]
    fn skill_output_and_error_extract() {
        let out = skill_planned_output("skill:x", &["a".into(), "b".into()]);
        assert_eq!(out["status"], json!("planned"));
        assert_eq!(out["dynamic_steps_generated"], json!(2));
        assert_eq!(out["__substep_ids"], json!(["a", "b"]));
        assert_eq!(
            step_output_error_message(&json!({ "error": "boom" })),
            Some("boom")
        );
        assert!(step_output_error_message(&json!({ "error": "" })).is_none());
        assert!(step_output_error_message(&json!({})).is_none());
    }
}
