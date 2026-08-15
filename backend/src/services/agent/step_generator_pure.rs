//! Pure step-generator projection and condition evaluation for the executor.
//!
//! AI calls / registry lookups stay in `executor/mod.rs`. Domain owns:
//! - dependency readiness against step outputs
//! - ConditionalBranch condition parsing and evaluation
//! - UiInteractionFromAnalysis / IterateFromList step materialization
//! - AI-generated JSON array extraction and step projection

use crate::services::agent::executor_resolve_pure::{is_truthy, resolve_dot_path};
use crate::services::agent::types::{FailureStrategy, RecipeStep};
use serde_json::Value;
use std::collections::HashMap;

/// Max UI interaction steps generated from analysis elements.
pub const MAX_UI_INTERACTION_STEPS: usize = 5;
/// Max items expanded by IterateFromList.
pub const MAX_ITERATE_ITEMS: usize = 10;
/// Max steps accepted from AiGenerated JSON.
pub const MAX_AI_GENERATED_STEPS: usize = 3;
/// Default timeout for UI interaction generated steps (ms).
pub const UI_STEP_TIMEOUT_MS: u64 = 15_000;
/// Default timeout for iterate / AI generated steps (ms).
pub const GENERATED_STEP_TIMEOUT_MS: u64 = 30_000;

/// Whether all `depends_on` step ids exist in previous outputs.
pub fn check_dependencies(step: &RecipeStep, outputs: &HashMap<String, Value>) -> bool {
    step.depends_on.iter().all(|dep_id| outputs.contains_key(dep_id))
}

/// Parsed condition operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionOp {
    Truthy,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

/// Split a condition string into path / operator / expected literal.
pub fn parse_condition(condition: &str) -> (String, ConditionOp, String) {
    let condition = condition.trim();
    if let Some(pos) = condition.find("!=") {
        let (p, v) = condition.split_at(pos);
        (p.trim().to_string(), ConditionOp::Ne, v[2..].trim().to_string())
    } else if let Some(pos) = condition.find("==") {
        let (p, v) = condition.split_at(pos);
        (p.trim().to_string(), ConditionOp::Eq, v[2..].trim().to_string())
    } else if let Some(pos) = condition.find(">=") {
        let (p, v) = condition.split_at(pos);
        (p.trim().to_string(), ConditionOp::Ge, v[2..].trim().to_string())
    } else if let Some(pos) = condition.find("<=") {
        let (p, v) = condition.split_at(pos);
        (p.trim().to_string(), ConditionOp::Le, v[2..].trim().to_string())
    } else if let Some(pos) = condition.find('>') {
        let (p, v) = condition.split_at(pos);
        (p.trim().to_string(), ConditionOp::Gt, v[1..].trim().to_string())
    } else if let Some(pos) = condition.find('<') {
        let (p, v) = condition.split_at(pos);
        (p.trim().to_string(), ConditionOp::Lt, v[1..].trim().to_string())
    } else {
        (condition.to_string(), ConditionOp::Truthy, String::new())
    }
}

/// Compare a resolved JSON value against a condition op + expected token.
pub fn compare_condition_value(value: &Value, op: &ConditionOp, expected: &str) -> bool {
    match op {
        ConditionOp::Truthy => is_truthy(value),
        ConditionOp::Eq => {
            if expected == "null" || expected == "nil" {
                value.is_null()
            } else if let Ok(expected_num) = expected.parse::<f64>() {
                value
                    .as_f64()
                    .is_some_and(|v| (v - expected_num).abs() < f64::EPSILON)
            } else {
                let expected_str = expected.trim_matches('"').trim_matches('\'');
                value.as_str() == Some(expected_str)
            }
        }
        ConditionOp::Ne => {
            if expected == "null" || expected == "nil" {
                !value.is_null()
            } else if let Ok(expected_num) = expected.parse::<f64>() {
                value
                    .as_f64()
                    .is_none_or(|v| (v - expected_num).abs() >= f64::EPSILON)
            } else {
                let expected_str = expected.trim_matches('"').trim_matches('\'');
                value.as_str() != Some(expected_str)
            }
        }
        ConditionOp::Gt | ConditionOp::Ge | ConditionOp::Lt | ConditionOp::Le => {
            let actual = value.as_f64().unwrap_or(0.0);
            let expected_num = expected.parse::<f64>().unwrap_or(0.0);
            match op {
                ConditionOp::Gt => actual > expected_num,
                ConditionOp::Ge => actual >= expected_num,
                ConditionOp::Lt => actual < expected_num,
                ConditionOp::Le => actual <= expected_num,
                _ => false,
            }
        }
    }
}

/// Evaluate a generator condition against current + prior step outputs.
///
/// Formats: `output.field`, `step_id.field`, `path == value`, comparisons,
/// `path != null`.
pub fn evaluate_condition(
    condition: &str,
    current_output: &Value,
    step_outputs: &HashMap<String, Value>,
) -> bool {
    let (path, op, expected) = parse_condition(condition);
    let value = resolve_dot_path(&path, current_output, step_outputs);
    compare_condition_value(&value, &op, &expected)
}

/// Pick list array from step output (`items` / `data` / `list`).
pub fn extract_list_items(output: &Value) -> Option<&Vec<Value>> {
    output
        .get("items")
        .or_else(|| output.get("data"))
        .or_else(|| output.get("list"))
        .and_then(|v| v.as_array())
}

/// Materialize UI interaction steps from analysis `elements` array.
pub fn generate_ui_interaction_steps(
    parent_step: &RecipeStep,
    elements: &[Value],
    operation_intent: &str,
) -> Vec<RecipeStep> {
    elements
        .iter()
        .take(MAX_UI_INTERACTION_STEPS)
        .enumerate()
        .map(|(i, el)| {
            let mut params = HashMap::new();
            params.insert("element".to_string(), el.clone());
            params.insert(
                "intent".to_string(),
                Value::String(operation_intent.to_string()),
            );
            RecipeStep {
                id: format!("{}_ui_{}", parent_step.id, i),
                order: parent_step.order + 1 + i as u32,
                capability_id: "tapp.interact".to_string(),
                action: "interact".to_string(),
                params,
                depends_on: vec![parent_step.id.clone()],
                on_failure: FailureStrategy::Skip,
                retry: None,
                timeout_ms: Some(UI_STEP_TIMEOUT_MS),
                model_tier: None,
                generator: None,
            }
        })
        .collect()
}

/// Materialize per-item steps from a list-shaped prior output.
pub fn generate_iterate_from_list_steps(
    parent_step: &RecipeStep,
    items: &[Value],
    item_capability: &str,
) -> Vec<RecipeStep> {
    items
        .iter()
        .take(MAX_ITERATE_ITEMS)
        .enumerate()
        .map(|(i, item)| {
            let mut params = HashMap::new();
            params.insert("item".to_string(), item.clone());
            params.insert(
                "index".to_string(),
                Value::Number(serde_json::Number::from(i)),
            );
            RecipeStep {
                id: format!("{}_iter_{}", parent_step.id, i),
                order: parent_step.order + 1 + i as u32,
                capability_id: item_capability.to_string(),
                action: "process".to_string(),
                params,
                depends_on: vec![parent_step.id.clone()],
                on_failure: FailureStrategy::Skip,
                retry: None,
                timeout_ms: Some(GENERATED_STEP_TIMEOUT_MS),
                model_tier: None,
                generator: None,
            }
        })
        .collect()
}

/// Extract the outermost JSON array slice from model text (markdown fences ok).
pub fn extract_json_array_slice(text: &str) -> &str {
    let text = text.trim();
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            return &text[start..=end];
        }
    }
    text
}

/// Project AI JSON array items into RecipeSteps (max [`MAX_AI_GENERATED_STEPS`]).
///
/// Items without `capability_id` are skipped.
pub fn project_ai_generated_steps(
    items: &[Value],
    parent_step: &RecipeStep,
) -> Vec<RecipeStep> {
    items
        .iter()
        .take(MAX_AI_GENERATED_STEPS)
        .enumerate()
        .filter_map(|(i, item)| {
            let id = item
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let cap_id = item.get("capability_id").and_then(|v| v.as_str())?.to_string();
            let action = item
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("process")
                .to_string();
            let params: HashMap<String, Value> = item
                .get("params")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            Some(RecipeStep {
                id: if id.is_empty() {
                    format!("{}_ai_{}", parent_step.id, i)
                } else {
                    id
                },
                order: parent_step.order + 1 + i as u32,
                capability_id: cap_id,
                action,
                params,
                depends_on: vec![parent_step.id.clone()],
                on_failure: FailureStrategy::Skip,
                retry: None,
                timeout_ms: Some(GENERATED_STEP_TIMEOUT_MS),
                model_tier: None,
                generator: None,
            })
        })
        .collect()
}

/// Parse model response text into generated steps (empty on JSON failure).
pub fn parse_ai_generated_steps_from_text(
    text: &str,
    parent_step: &RecipeStep,
) -> Result<Vec<RecipeStep>, String> {
    let json_str = extract_json_array_slice(text);
    let items: Vec<Value> =
        serde_json::from_str(json_str).map_err(|e| format!("parse AI steps JSON: {e}"))?;
    Ok(project_ai_generated_steps(&items, parent_step))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parent(id: &str, order: u32) -> RecipeStep {
        RecipeStep {
            id: id.into(),
            order,
            capability_id: "test.cap".into(),
            action: "run".into(),
            params: HashMap::new(),
            depends_on: vec![],
            on_failure: FailureStrategy::Abort,
            retry: None,
            timeout_ms: None,
            model_tier: None,
            generator: None,
        }
    }

    #[test]
    fn dependencies_require_all_outputs() {
        let mut step = parent("s2", 2);
        step.depends_on = vec!["s1".into(), "s0".into()];
        let mut outs = HashMap::new();
        outs.insert("s1".into(), json!(1));
        assert!(!check_dependencies(&step, &outs));
        outs.insert("s0".into(), json!(0));
        assert!(check_dependencies(&step, &outs));
    }

    #[test]
    fn condition_truthy_eq_and_compare() {
        let mut outs = HashMap::new();
        outs.insert("prev".into(), json!({ "n": 3, "name": "ok" }));
        let cur = json!({ "flag": true, "score": 10, "x": null });

        assert!(evaluate_condition("output.flag", &cur, &outs));
        assert!(!evaluate_condition("output.missing", &cur, &outs));
        assert!(evaluate_condition("prev.n == 3", &cur, &outs));
        assert!(evaluate_condition("prev.name == \"ok\"", &cur, &outs));
        assert!(evaluate_condition("output.score > 5", &cur, &outs));
        assert!(evaluate_condition("output.score >= 10", &cur, &outs));
        assert!(!evaluate_condition("output.score < 5", &cur, &outs));
        assert!(!evaluate_condition("output.x != null", &cur, &outs));
        assert!(evaluate_condition("output.x == null", &cur, &outs));
        assert!(evaluate_condition("prev.n != 9", &cur, &outs));
    }

    #[test]
    fn parse_condition_ops_order() {
        // Multi-char ops must win over single-char
        let (_, op, exp) = parse_condition("a.b >= 2");
        assert_eq!(op, ConditionOp::Ge);
        assert_eq!(exp, "2");
        let (_, op, _) = parse_condition("a != null");
        assert_eq!(op, ConditionOp::Ne);
    }

    #[test]
    fn ui_and_iterate_caps_and_limits() {
        let p = parent("analyze", 1);
        let elements: Vec<Value> = (0..8).map(|i| json!({ "id": i })).collect();
        let ui = generate_ui_interaction_steps(&p, &elements, "click");
        assert_eq!(ui.len(), MAX_UI_INTERACTION_STEPS);
        assert_eq!(ui[0].capability_id, "tapp.interact");
        assert_eq!(ui[0].depends_on, vec!["analyze"]);
        assert_eq!(ui[0].timeout_ms, Some(UI_STEP_TIMEOUT_MS));
        assert_eq!(ui[2].id, "analyze_ui_2");

        let items: Vec<Value> = (0..15).map(|i| json!(i)).collect();
        let iters = generate_iterate_from_list_steps(&p, &items, "data.process");
        assert_eq!(iters.len(), MAX_ITERATE_ITEMS);
        assert_eq!(iters[0].capability_id, "data.process");
        assert_eq!(iters[0].params.get("index"), Some(&json!(0)));
        assert_eq!(iters[9].id, "analyze_iter_9");
    }

    #[test]
    fn extract_list_prefers_items_then_data_list() {
        assert_eq!(
            extract_list_items(&json!({ "items": [1], "data": [2] }))
                .map(|a| a.len()),
            Some(1)
        );
        assert_eq!(
            extract_list_items(&json!({ "data": [1, 2] })).map(|a| a.len()),
            Some(2)
        );
        assert_eq!(
            extract_list_items(&json!({ "list": [1, 2, 3] })).map(|a| a.len()),
            Some(3)
        );
        assert!(extract_list_items(&json!({ "x": 1 })).is_none());
    }

    #[test]
    fn ai_json_slice_and_projection() {
        let text = "here:\n```json\n[{\"id\":\"g1\",\"capability_id\":\"ai.chat\",\"action\":\"say\",\"params\":{\"q\":\"hi\"}},{\"capability_id\":\"http.fetch\"},{\"foo\":1}]\n```";
        let slice = extract_json_array_slice(text);
        assert!(slice.starts_with('['));
        let p = parent("parent", 5);
        let steps = parse_ai_generated_steps_from_text(text, &p).expect("parse");
        // third item lacks capability_id; first two kept, max 3 but only 2 valid
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].id, "g1");
        assert_eq!(steps[0].order, 6);
        assert_eq!(steps[0].params.get("q"), Some(&json!("hi")));
        assert_eq!(steps[1].id, "parent_ai_1");
        assert_eq!(steps[1].action, "process");
    }

    #[test]
    fn ai_steps_cap_at_three() {
        let p = parent("p", 0);
        let items: Vec<Value> = (0..5)
            .map(|i| json!({ "capability_id": format!("c{i}") }))
            .collect();
        assert_eq!(project_ai_generated_steps(&items, &p).len(), 3);
    }
}
