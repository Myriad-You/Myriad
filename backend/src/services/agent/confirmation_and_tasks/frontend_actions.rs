// Work result assembly: final result and frontend actions.

use serde_json::Value;

use super::super::agent_header::*;

impl Agent {
    /// 从执行结果中提取前端动作
    pub(crate) fn extract_frontend_action(&self, result: &Value) -> Option<Value> {
        extract_frontend_action_from_result(result)
    }
}

fn typed_frontend_action(value: &Value) -> Option<Value> {
    match value {
        Value::Object(map) => map
            .get("type")
            .and_then(Value::as_str)
            .filter(|kind| !kind.is_empty())
            .map(|_| value.clone()),
        _ => None,
    }
}

/// Collect executable frontend actions from step outputs.
///
/// Prefer `frontendActions` array; else typed `frontendAction`; else typed `action`.
/// Skip nulls and bare strings.
pub(crate) fn collect_step_frontend_actions<'a, I>(outputs: I) -> Vec<Value>
where
    I: IntoIterator<Item = &'a Value>,
{
    let mut actions = Vec::new();
    for output in outputs {
        if let Some(list) = output.get("frontendActions").and_then(Value::as_array) {
            for item in list {
                if let Some(action) = typed_frontend_action(item) {
                    actions.push(action);
                }
            }
            continue;
        }
        if let Some(action) = output.get("frontendAction").and_then(typed_frontend_action) {
            actions.push(action);
        } else if let Some(action) = output.get("action").and_then(typed_frontend_action) {
            actions.push(action);
        }
    }
    actions
}

/// Pull a frontend action out of a step/final result.
///
/// `tapp.understand` sets `frontendAction: null` (analysis is not executable).
/// This extractor does not read `plan.steps` or synthesize navigate.
pub(crate) fn extract_frontend_action_from_result(result: &Value) -> Option<Value> {
    if let Some(action) = result.get("frontendAction") {
        if action.get("type").and_then(Value::as_str).is_some() {
            return Some(action.clone());
        }
        if !action.is_null() {
            tracing::warn!(action = %action, "[Agent] frontendAction is missing type");
        }
        return None;
    }

    if let Some(action) = result.get("action") {
        if action.get("type").and_then(Value::as_str).is_some() {
            let mut final_action = action.clone();
            if final_action.get("criteria").is_none() {
                if let Some(criteria) = result.get("criteria") {
                    final_action["criteria"] = criteria.clone();
                }
            }
            return Some(final_action);
        }
    }

    if let Some(actions) = result.get("frontendActions").and_then(|v| v.as_array()) {
        if let Some(first_action) = actions.first() {
            if first_action.get("type").and_then(Value::as_str).is_some() {
                return Some(first_action.clone());
            }
        }
    }

    None
}

#[cfg(test)]
mod extract_frontend_action_tests {
    use super::{collect_step_frontend_actions, extract_frontend_action_from_result};
    use serde_json::json;

    #[test]
    fn understand_null_frontend_action_does_not_synthesize_clicks() {
        let result = json!({
            "frontendAction": null,
            "plan": {
                "canFulfill": true,
                "steps": [{ "actionType": "click", "target": { "text": "保存" } }]
            }
        });
        assert_eq!(extract_frontend_action_from_result(&result), None);
    }

    #[test]
    fn typed_frontend_action_is_returned() {
        let result = json!({
            "frontendAction": { "type": "navigate", "path": "/library" }
        });
        let action = extract_frontend_action_from_result(&result).unwrap();
        assert_eq!(action["type"], "navigate");
        assert_eq!(action["path"], "/library");
    }

    #[test]
    fn reading_list_action_field_is_still_collected() {
        let result = json!({
            "action": { "type": "reading_list", "payload": { "items": [] } },
            "criteria": "科幻"
        });
        let action = extract_frontend_action_from_result(&result).unwrap();
        assert_eq!(action["type"], "reading_list");
        assert_eq!(action["criteria"], "科幻");
    }

    #[test]
    fn collect_skips_string_action_and_null_frontend_action() {
        let music = json!({
            "action": "play",
            "frontendAction": { "type": "music_control", "action": "play" }
        });
        let understand = json!({
            "frontendAction": null,
            "plan": { "steps": [] }
        });
        let reading = json!({
            "action": { "type": "reading_list", "payload": { "items": [] } }
        });
        let collected = collect_step_frontend_actions([&music, &understand, &reading]);
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0]["type"], "music_control");
        assert_eq!(collected[1]["type"], "reading_list");
    }

    #[test]
    fn collect_prefers_frontend_actions_array() {
        let plan = json!({
            "frontendActions": [
                { "type": "page_interact", "action": "click" },
                { "type": "navigate", "path": "/library" }
            ],
            "frontendAction": { "type": "page_interact", "action": "click" }
        });
        let collected = collect_step_frontend_actions([&plan]);
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[1]["type"], "navigate");
    }
}
