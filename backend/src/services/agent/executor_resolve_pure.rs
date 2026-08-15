//! Pure helpers for executor_resolve_pure.


use crate::services::agent::executor_utils_pure::truncate_str;
use crate::services::agent::types::{QuestionType, RiskLevel, UserQuestion};
use crate::services::agent::SYSTEM_USER_ID;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Max string length kept in StepDebug param previews.
pub const DEBUG_PARAM_MAX_CHARS: usize = 500;

/// Whether an unconfirmed dynamically-generated step must be blocked.
///
/// Aligns with [`Agent::system_sensitive_gate`]:
/// - **System / heartbeat** (`SYSTEM_USER_ID`): Medium auto-runs (same as plan-time
/// gate); High / Critical hard-block with a clear error (not silent skip).
/// - **Interactive users**: Medium and above block until confirmed.
///
/// Only Low may auto-run for normal users without an extra confirmation gate.
pub fn should_block_unconfirmed_dynamic_step(user_id: i32, risk: RiskLevel) -> bool {
    if user_id == SYSTEM_USER_ID {
        matches!(risk, RiskLevel::High | RiskLevel::Critical)
    } else {
        matches!(
            risk,
            RiskLevel::Medium | RiskLevel::High | RiskLevel::Critical
        )
    }
}

/// Truncate long string params for StepDebug events.
pub fn build_debug_params(params: &HashMap<String, Value>) -> Option<Value> {
    let mut p = params.clone();
    for v in p.values_mut() {
        if let Some(s) = v.as_str() {
            if s.len() > DEBUG_PARAM_MAX_CHARS {
                *v = json!(format!(
                    "{}...({}chars)",
                    truncate_str(s, DEBUG_PARAM_MAX_CHARS),
                    s.len()
                ));
            }
        }
    }
    serde_json::to_value(&p).ok()
}

/// Data-shaped params keep full objects for handlers that parse structure.
pub fn is_data_param_key(key: &str) -> bool {
    matches!(
        key,
        "data" | "content" | "input" | "context" | "items"
    )
}

/// ID-shaped params must never fall through to semantic text extraction.
pub fn is_id_param_key(key: &str) -> bool {
    key == "id" || key.ends_with("Id") || key.ends_with("ID") || key.ends_with("_id")
}

/// Whether params already carry a non-empty string/number id under `key`.
pub fn has_nonempty_id_param(params: &HashMap<String, Value>, key: &str) -> bool {
    params
        .get(key)
        .map(|v| {
            v.as_str().map(|s| !s.is_empty()).unwrap_or(false)
                || v.as_i64().is_some()
                || v.as_u64().is_some()
        })
        .unwrap_or(false)
}

/// Search prior step outputs for a playlist id (recommended or playlists[0]).
pub fn find_playlist_id_in_outputs(previous_outputs: &HashMap<String, Value>) -> Option<String> {
    for output in previous_outputs.values() {
        if let Some(pid) = output
            .get("recommendedPlaylistId")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            return Some(pid.to_string());
        }
        if let Some(first) = output
            .get("playlists")
            .and_then(|p| p.as_array())
            .and_then(|arr| arr.first())
        {
            let id_opt = first.get("id").and_then(|id| {
                id.as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| id.as_str().map(String::from))
            });
            if let Some(id_str) = id_opt {
                return Some(id_str);
            }
        }
    }
    None
}

/// Resolve step params: expand `*From` references against previous outputs.
///
/// Returns `(resolved_params, unresolved_ref_labels)`.
pub fn resolve_params(
    params: &HashMap<String, Value>,
    previous_outputs: &HashMap<String, Value>,
) -> (HashMap<String, Value>, Vec<String>) {
    let mut resolved = HashMap::new();
    let mut unresolved = Vec::new();

    for (key, value) in params {
        if key.ends_with("From") {
            if let Some(ref_str) = value.as_str() {
                let resolved_value = resolve_path_reference(ref_str, previous_outputs);
                if let Some(output) = resolved_value {
                    let new_key = key.trim_end_matches("From").to_string();
                    let is_data_param = is_data_param_key(&new_key);
                    let is_id_param = is_id_param_key(&new_key);

                    let final_value = if output.is_string() || is_data_param {
                        Some(output)
                    } else if is_id_param && (output.is_object() || output.is_array()) {
                        extract_id_from_output(&output, &new_key)
                    } else if output.is_object() {
                        let text = extract_text_from_output(&output);
                        if text.is_empty() {
                            Some(output)
                        } else {
                            Some(Value::String(text))
                        }
                    } else {
                        Some(output)
                    };

                    match final_value {
                        Some(v) => {
                            resolved.insert(new_key, v);
                            continue;
                        }
                        None => unresolved.push(format!("{key}: {ref_str}")),
                    }
                } else {
                    unresolved.push(format!("{key}: {ref_str}"));
                }
            }
        }

        resolved.insert(key.clone(), value.clone());
    }

    (resolved, unresolved)
}

/// Extract an ID-typed value from structured step output.
///
/// Priority: same-name field → top-level `id` → first item in common list keys.
/// Array input recurses into the first element.
pub fn extract_id_from_output(output: &Value, param_key: &str) -> Option<Value> {
    fn id_like(v: &Value) -> bool {
        v.is_string() || v.is_number()
    }

    match output {
        Value::Array(arr) => arr
            .first()
            .and_then(|first| extract_id_from_output(first, param_key)),
        Value::Object(obj) => {
            if let Some(v) = obj.get(param_key).filter(|v| id_like(v)) {
                return Some(v.clone());
            }
            if let Some(v) = obj.get("id").filter(|v| id_like(v)) {
                return Some(v.clone());
            }
            for list_key in ["playlists", "results", "items", "list", "songs", "data"] {
                if let Some(first) = obj
                    .get(list_key)
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                {
                    if let Some(v) = first.get(param_key).filter(|v| id_like(v)) {
                        return Some(v.clone());
                    }
                    if let Some(v) = first.get("id").filter(|v| id_like(v)) {
                        return Some(v.clone());
                    }
                }
            }
            None
        }
        _ => None,
    }
}

/// Prefer primary text fields from a step output object.
pub fn extract_text_from_output(output: &Value) -> String {
    let text_keys = [
        "analysis",
        "reply",
        "aiSummary",
        "summary",
        "description",
        "message",
        "content",
        "prompt",
    ];
    if let Some(obj) = output.as_object() {
        for key in &text_keys {
            if let Some(text) = obj.get(*key).and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    return text.to_string();
                }
            }
        }
    }
    String::new()
}

/// Resolve `step_id` or `step_id.path.to.value` / `step_id.array[0].field`.
///
/// Skill placeholders with `__substep_ids` substitute the last completed sub-step output.
pub fn resolve_path_reference(
    ref_str: &str,
    previous_outputs: &HashMap<String, Value>,
) -> Option<Value> {
    let parts: Vec<&str> = ref_str.splitn(2, '.').collect();
    let step_id = parts[0];

    let output = previous_outputs.get(step_id)?;

    let effective_output =
        if let Some(substep_ids) = output.get("__substep_ids").and_then(|v| v.as_array()) {
            let mut last: Option<&Value> = None;
            for id_val in substep_ids {
                if let Some(id) = id_val.as_str() {
                    if let Some(sub_output) = previous_outputs.get(id) {
                        last = Some(sub_output);
                    }
                }
            }
            match last {
                Some(sub_out) => std::borrow::Cow::Borrowed(sub_out),
                None => std::borrow::Cow::Borrowed(output),
            }
        } else {
            std::borrow::Cow::Borrowed(output)
        };

    if parts.len() == 1 {
        return Some(effective_output.into_owned());
    }

    get_value_by_path(&effective_output, parts[1])
}

/// Walk a dotted path with optional `[index]` segments.
pub fn get_value_by_path(value: &Value, path: &str) -> Option<Value> {
    let mut current = value;

    for segment in path.split('.') {
        if let Some(bracket_pos) = segment.find('[') {
            let field_name = &segment[..bracket_pos];
            if !segment.ends_with(']') || bracket_pos + 1 >= segment.len() - 1 {
                return None;
            }
            let index_str = &segment[bracket_pos + 1..segment.len() - 1];

            if !field_name.is_empty() {
                current = current.get(field_name)?;
            }

            let index: usize = index_str.parse().ok()?;
            current = current.get(index)?;
        } else {
            current = current.get(segment)?;
        }
    }

    Some(current.clone())
}

/// Read a generator/condition dot-path from current or prior step outputs.
///
/// - `output.xxx` → current step output
/// - `step_id.xxx` → named prior output when key exists
/// - bare path → current step output
pub fn resolve_dot_path(
    path: &str,
    current_output: &Value,
    step_outputs: &HashMap<String, Value>,
) -> Value {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return Value::Null;
    }

    let (root, field_start) = if parts[0] == "output" {
        (current_output, 1)
    } else if step_outputs.contains_key(parts[0]) {
        match step_outputs.get(parts[0]) {
            Some(v) => (v, 1),
            None => return Value::Null,
        }
    } else {
        (current_output, 0)
    };

    let mut current = root.clone();
    for part in &parts[field_start..] {
        current = if let Ok(idx) = part.parse::<usize>() {
            current
                .as_array()
                .and_then(|arr| arr.get(idx).cloned())
                .unwrap_or(Value::Null)
        } else {
            current.get(part).cloned().unwrap_or(Value::Null)
        };
    }
    current
}

/// JSON truthiness used by condition generators.
pub fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|v| v != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(arr) => !arr.is_empty(),
        Value::Object(obj) => !obj.is_empty(),
    }
}

/// Project a successful Tapp interaction payload into a wait-for-input question.
///
/// `created_at` is injected by the caller so pure tests stay deterministic.
pub fn tapp_interaction_wait_question(
    output: &Value,
    created_at: DateTime<Utc>,
) -> Option<UserQuestion> {
    let interaction = output.get("interaction")?;
    let interaction_id = interaction
        .get("interactionId")
        .or_else(|| interaction.get("interaction_id"))?
        .as_str()?;
    let expires_at = interaction
        .get("deadline")
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc));
    Some(UserQuestion {
        question_id: format!("tapp_interaction:{interaction_id}"),
        question_type: QuestionType::FreeText,
        question: "等待 Tapp 完成交互".to_string(),
        context: format!(
            "Tapp Agent Interaction {interaction_id} 将在提交结构化结果后自动恢复此任务"
        ),
        options: None,
        required: true,
        default_value: None,
        created_at,
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dynamic_risk_gate_aligns_with_system_sensitive_gate() {
        // System/heartbeat: Medium auto-run; High+ blocked (matches system_sensitive_gate).
        assert!(should_block_unconfirmed_dynamic_step(
            SYSTEM_USER_ID,
            RiskLevel::Critical
        ));
        assert!(should_block_unconfirmed_dynamic_step(
            SYSTEM_USER_ID,
            RiskLevel::High
        ));
        assert!(!should_block_unconfirmed_dynamic_step(
            SYSTEM_USER_ID,
            RiskLevel::Medium
        ));
        assert!(!should_block_unconfirmed_dynamic_step(
            SYSTEM_USER_ID,
            RiskLevel::Low
        ));
        // Interactive: Medium+ blocked until confirmed.
        assert!(should_block_unconfirmed_dynamic_step(7, RiskLevel::High));
        assert!(should_block_unconfirmed_dynamic_step(7, RiskLevel::Medium));
        assert!(!should_block_unconfirmed_dynamic_step(7, RiskLevel::Low));
    }

    #[test]
    fn id_param_extracts_from_search_output() {
        let output = json!({
            "success": true,
            "message": "找到 10 个「凉宫春日」相关歌单",
            "keyword": "凉宫春日",
            "playlists": [
                { "id": 12597740641u64, "name": "悲情篇章" },
                { "id": 12764048642u64, "name": "アニサマ" }
            ]
        });
        let got = extract_id_from_output(&output, "playlistId");
        assert_eq!(got, Some(json!(12597740641u64)));
    }

    #[test]
    fn id_param_prefers_same_name_field() {
        let output = json!({ "playlistId": "abc123", "id": "other", "message": "文案" });
        let got = extract_id_from_output(&output, "playlistId");
        assert_eq!(got, Some(json!("abc123")));
    }

    #[test]
    fn id_param_falls_back_to_top_level_id() {
        let output = json!({ "id": 42, "message": "文案" });
        assert_eq!(
            extract_id_from_output(&output, "songId"),
            Some(json!(42))
        );
    }

    #[test]
    fn id_param_array_input_takes_first_element() {
        let output = json!([{ "id": "first" }, { "id": "second" }]);
        assert_eq!(
            extract_id_from_output(&output, "itemId"),
            Some(json!("first"))
        );
    }

    #[test]
    fn id_param_without_id_yields_none() {
        let output = json!({ "message": "找到 10 个歌单", "success": true });
        assert_eq!(extract_id_from_output(&output, "playlistId"), None);
    }

    #[test]
    fn resolve_params_id_from_search_object() {
        let mut previous = HashMap::new();
        previous.insert(
            "search".into(),
            json!({
                "message": "找到 10 个歌单",
                "playlists": [{ "id": 99, "name": "x" }]
            }),
        );
        let mut params = HashMap::new();
        params.insert("playlistIdFrom".into(), json!("search"));
        let (resolved, unresolved) = resolve_params(&params, &previous);
        assert!(unresolved.is_empty());
        assert_eq!(resolved.get("playlistId"), Some(&json!(99)));
        assert!(!resolved.contains_key("playlistIdFrom"));
    }

    #[test]
    fn resolve_params_unresolved_missing_step_and_idless_object() {
        let previous = HashMap::new();
        let mut params = HashMap::new();
        params.insert("playlistIdFrom".into(), json!("missing_step"));
        let (resolved, unresolved) = resolve_params(&params, &previous);
        assert!(unresolved.iter().any(|u| u.contains("missing_step")));
        assert!(!resolved.contains_key("playlistId"));

        let mut previous = HashMap::new();
        previous.insert("search".into(), json!({ "message": "找到 10 个歌单", "success": true }));
        let mut params = HashMap::new();
        params.insert("playlistIdFrom".into(), json!("search"));
        let (resolved, unresolved) = resolve_params(&params, &previous);
        assert!(!unresolved.is_empty());
        assert!(!resolved.contains_key("playlistId"));
    }

    #[test]
    fn resolve_params_data_from_keeps_object() {
        let mut previous = HashMap::new();
        previous.insert("s1".into(), json!({ "results": [1, 2], "message": "hi" }));
        let mut params = HashMap::new();
        params.insert("dataFrom".into(), json!("s1"));
        let (resolved, unresolved) = resolve_params(&params, &previous);
        assert!(unresolved.is_empty());
        assert_eq!(
            resolved.get("data"),
            Some(&json!({ "results": [1, 2], "message": "hi" }))
        );
    }

    #[test]
    fn path_walk_and_skill_substep_substitution() {
        let mut outs = HashMap::new();
        outs.insert(
            "skill".into(),
            json!({ "status": "planned", "__substep_ids": ["s1", "s2"] }),
        );
        outs.insert("s1".into(), json!({ "partial": true }));
        outs.insert(
            "s2".into(),
            json!({ "items": [{ "id": "a" }, { "id": "b" }], "message": "done" }),
        );
        let whole = resolve_path_reference("skill", &outs).expect("whole");
        assert_eq!(whole.get("message").and_then(|v| v.as_str()), Some("done"));
        let id = resolve_path_reference("skill.items[0].id", &outs);
        assert_eq!(id, Some(json!("a")));
    }

    #[test]
    fn playlist_fallback_and_text_truthy_dot() {
        let mut outs = HashMap::new();
        outs.insert(
            "rec".into(),
            json!({ "recommendedPlaylistId": "pl_1" }),
        );
        assert_eq!(find_playlist_id_in_outputs(&outs).as_deref(), Some("pl_1"));

        assert_eq!(
            extract_text_from_output(&json!({ "message": "hello", "id": 1 })),
            "hello"
        );
        assert!(!is_truthy(&Value::Null));
        assert!(is_truthy(&json!("x")));
        assert!(!is_truthy(&json!("")));
        assert!(is_truthy(&json!([1])));
        assert!(!is_truthy(&json!([])));

        let mut step_outs = HashMap::new();
        step_outs.insert("prev".into(), json!({ "n": 3 }));
        assert_eq!(
            resolve_dot_path("prev.n", &json!({}), &step_outs),
            json!(3)
        );
        assert_eq!(
            resolve_dot_path("output.x", &json!({ "x": true }), &step_outs),
            json!(true)
        );
    }

    #[test]
    fn debug_params_truncate_long_strings() {
        let mut params = HashMap::new();
        let long = "a".repeat(600);
        params.insert("prompt".into(), json!(long));
        let preview = build_debug_params(&params).expect("preview");
        let s = preview
            .get("prompt")
            .and_then(|v| v.as_str())
            .expect("str");
        assert!(s.contains("...("));
        assert!(s.len() < 600);
    }

    #[test]
    fn interaction_wait_question_projects_id_and_deadline() {
        let now = Utc::now();
        let q = tapp_interaction_wait_question(
            &json!({
                "interaction": {
                    "interactionId": "ix_9",
                    "deadline": "2030-01-01T00:00:00Z"
                }
            }),
            now,
        )
        .expect("question");
        assert_eq!(q.question_id, "tapp_interaction:ix_9");
        assert!(q.expires_at.is_some());
        assert_eq!(q.created_at, now);
        assert!(tapp_interaction_wait_question(&json!({}), now).is_none());
    }
}
