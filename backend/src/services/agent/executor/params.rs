//! Step parameter and output helpers shared by the Work loop: resolving
//! `*From` references between recipe steps, filling request context, and the
//! declared output contract.

use crate::services::agent::capability::CapabilityRef;
use crate::services::agent::types::*;
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use std::collections::HashMap;

/// Breach (wrong JSON type / missing required) fails the step.
/// `ContractViolation::Drift` 只 `tracing::warn` 后 `Ok(())`。
///
/// MCP output schemas use the Work loop's JSON Schema validator; this
/// builtin contract checker does not interpret their dynamic schemas.
pub(crate) fn apply_output_contract(
    step: &RecipeStep,
    capability: &Capability,
    output: &Value,
) -> Result<(), String> {
    if CapabilityRef::parse(&step.capability_id).is_mcp() {
        return Ok(());
    }

    let Some(violation) = crate::services::agent::capability::check_output_contract(
        &capability.output_schema,
        output,
    ) else {
        return Ok(());
    };

    if violation.is_fatal() {
        tracing::error!(
            step_id = %step.id,
            capability = %step.capability_id,
            violation = violation.message(),
            "[Work] Step output breaches its declared contract"
        );
        return Err(violation.message().to_string());
    }
    tracing::warn!(
        step_id = %step.id,
        capability = %step.capability_id,
        violation = violation.message(),
        "[Work] Capability output_schema has drifted from its handler"
    );
    Ok(())
}

pub(crate) fn resolve_params(
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

                    // 数据型参数（data, content, input, context, items）保留完整对象，
                    // 因为下游 handler（如 ai.analyze）有自己的 extract_semantic_text 逻辑
                    // 能处理结构化数据（如 results 数组、嵌套字段等）。
                    //
                    // 文本型参数（title, description, summary, prompt 等）提取语义文本，
                    // 因为下游期望的是纯字符串。
                    let is_data_param = matches!(
                        new_key.as_str(),
                        "data" | "content" | "input" | "context" | "items"
                    );

                    // ID 型参数（playlistId、songId、id 等）：引用结构化输出时
                    // 必须提取真实 ID，不能走语义文本提取。
                    let is_id_param = new_key == "id"
                        || new_key.ends_with("Id")
                        || new_key.ends_with("ID")
                        || new_key.ends_with("_id");

                    let final_value = if output.is_string() || is_data_param {
                        Some(output)
                    } else if is_id_param && (output.is_object() || output.is_array()) {
                        // 提取失败按未解析处理：让步骤显式报错，
                        // 而不是塞进错误的值静默错下去
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
                        None => unresolved.push(format!("{}: {}", key, ref_str)),
                    }
                } else {
                    unresolved.push(format!("{}: {}", key, ref_str));
                }
            }
        }

        resolved.insert(key.clone(), value.clone());
    }

    (resolved, unresolved)
}

/// 从结构化步骤输出中提取 ID 型参数值。
/// 优先级：同名字段 → 顶层 id → 常见列表字段第一项的 id / 同名字段。
/// 数组输入取第一个元素递归提取
pub(crate) fn extract_id_from_output(output: &Value, param_key: &str) -> Option<Value> {
    fn id_like(v: &Value) -> bool {
        v.is_string() || v.is_number()
    }

    match output {
        Value::Array(arr) => arr
            .first()
            .and_then(|first| extract_id_from_output(first, param_key)),
        Value::Object(obj) => {
            // 1. 同名字段（如 playlistId）
            if let Some(v) = obj.get(param_key).filter(|v| id_like(v)) {
                return Some(v.clone());
            }
            // 2. 顶层 id
            if let Some(v) = obj.get("id").filter(|v| id_like(v)) {
                return Some(v.clone());
            }
            // 3. 常见列表字段的第一项（搜索类输出：playlists/results/…）
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

/// 从步骤输出的 JSON 对象中提取主要文本内容
pub(crate) fn extract_text_from_output(output: &Value) -> String {
    let output = crate::services::agent::ai_process_pure::task_inner_value(output);
    if let Some(text) = output.as_str().filter(|s| !s.is_empty()) {
        return text.to_string();
    }
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

/// 解析路径引用
pub(crate) fn resolve_path_reference(
    ref_str: &str,
    previous_outputs: &HashMap<String, Value>,
) -> Option<Value> {
    // 支持 "step_id" 或 "step_id.path.to.value" 或 "step_id.array[0].field"
    let parts: Vec<&str> = ref_str.splitn(2, '.').collect();
    let step_id = parts[0];

    let output = previous_outputs.get(step_id)?;

    // Skill 步骤特殊处理：输出中 __substep_ids 表示这是技能占位符，
    // 用最后一个已完成子步骤的真实输出替代无意义的 "planned" 元数据
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

    // 解析路径
    let path = parts[1];
    get_value_by_path(&effective_output, path)
}

/// 通过路径获取值。信封步骤先按外壳取，没有再拆 `value`。
pub(crate) fn get_value_by_path(value: &Value, path: &str) -> Option<Value> {
    lookup_path(value, path).or_else(|| {
        lookup_path(
            crate::services::agent::ai_process_pure::task_inner_value(value),
            path,
        )
    })
}

fn lookup_path(value: &Value, path: &str) -> Option<Value> {
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

/// Fill `currentPath` / `context` from the turn request when the planner omitted
/// them. `router.state` declares an empty input schema, so without this it
/// always reports `/`.
pub(crate) fn inject_request_context_params(
    capability_id: &str,
    params: &mut HashMap<String, Value>,
    current_route: Option<&Value>,
    page_context: Option<&Value>,
    music_status: Option<&Value>,
    window_state: Option<&Value>,
) {
    let needs_path = matches!(
        capability_id,
        "router.state" | "page.content" | "page.understand" | "page.interact"
    );
    if needs_path {
        let missing = params
            .get("currentPath")
            .is_none_or(|v| v.as_str().map(str::trim).unwrap_or("").is_empty());
        if missing {
            if let Some(route) = current_route
                .cloned()
                .filter(|v| v.as_str().map(str::trim).is_some_and(|s| !s.is_empty()))
            {
                params.insert("currentPath".to_string(), route);
            }
        }
    }
    if matches!(capability_id, "page.content" | "page.understand")
        && !params.contains_key("context")
    {
        if let Some(page) = page_context.cloned() {
            params.insert("context".to_string(), page);
        }
    }
    if capability_id == "music.status" && !params.contains_key("status") {
        if let Some(status) = music_status.cloned() {
            params.insert("status".to_string(), status);
        }
    }
    if capability_id == "tapp.windows" && !params.contains_key("windowState") {
        if let Some(state) = window_state.cloned() {
            params.insert("windowState".to_string(), state);
        }
    }
}

#[cfg(test)]
mod output_contract_tests {
    use super::*;

    fn step(capability_id: &str) -> RecipeStep {
        RecipeStep {
            id: "s1".to_string(),
            order: 0,
            capability_id: capability_id.to_string(),
            action: String::new(),
            params: HashMap::new(),
            depends_on: vec![],
            on_failure: FailureStrategy::Abort,
            retry: None,
            timeout_ms: None,
            model_tier: None,
            generator: None,
        }
    }

    fn capability(capability_id: &str, output_schema: Value) -> Capability {
        Capability {
            id: capability_id.to_string(),
            output_schema,
            ..Default::default()
        }
    }

    fn summarize_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "keyPoints": { "type": "array" }
            }
        })
    }

    #[test]
    fn breach_fails_the_step_and_drift_does_not() {
        let cap = capability("ai.summarize", summarize_schema());
        let id = step("ai.summarize");
        assert!(apply_output_contract(&id, &cap, &json!({ "summary": "ok" })).is_ok());
        assert!(apply_output_contract(&id, &cap, &json!({ "summary": 42 })).is_err());
        assert!(
            apply_output_contract(&id, &cap, &json!({ "message": "no declared field" })).is_ok()
        );
        assert!(apply_output_contract(&id, &cap, &json!({})).is_ok());
        assert!(apply_output_contract(&id, &cap, &json!("not an object")).is_err());
    }

    #[test]
    fn extract_text_and_path_unwrap_task_envelope() {
        let envelope = json!({
            "format": "json",
            "value": { "analysis": "分析正文", "type": "custom" },
            "contextProvenance": []
        });
        assert_eq!(extract_text_from_output(&envelope), "分析正文");
        assert_eq!(
            extract_text_from_output(&json!({
                "format": "text",
                "value": "回复正文",
                "contextProvenance": []
            })),
            "回复正文"
        );
        assert_eq!(
            get_value_by_path(&envelope, "analysis").and_then(|v| v.as_str().map(str::to_string)),
            Some("分析正文".to_string())
        );
        assert_eq!(
            get_value_by_path(&envelope, "value.analysis")
                .and_then(|v| v.as_str().map(str::to_string)),
            Some("分析正文".to_string())
        );
        assert_eq!(
            get_value_by_path(&envelope, "format").and_then(|v| v.as_str().map(str::to_string)),
            Some("json".to_string())
        );
    }

    #[test]
    fn mcp_tools_are_exempt_from_the_contract() {
        assert!(
            apply_output_contract(
                &step("mcp.docs.lookup"),
                &capability("mcp.docs.lookup", json!({ "type": "string" })),
                &json!({ "content": [{ "type": "text" }] }),
            )
            .is_ok()
        );
    }

    #[test]
    fn request_route_fills_router_state_current_path() {
        let mut params = HashMap::new();
        inject_request_context_params(
            "router.state",
            &mut params,
            Some(&json!("/library")),
            None,
            None,
            None,
        );
        assert_eq!(
            params.get("currentPath").and_then(Value::as_str),
            Some("/library")
        );
    }

    #[test]
    fn page_snapshot_fills_page_content_context() {
        let mut params = HashMap::new();
        let snapshot = json!({ "content": "正文", "title": "标题" });
        inject_request_context_params(
            "page.content",
            &mut params,
            Some(&json!("/phantasi")),
            Some(&snapshot),
            None,
            None,
        );
        assert_eq!(
            params.get("currentPath").and_then(Value::as_str),
            Some("/phantasi")
        );
        assert_eq!(params.get("context"), Some(&snapshot));
    }

    #[test]
    fn explicit_current_path_is_not_overwritten() {
        let mut params = HashMap::new();
        params.insert("currentPath".to_string(), json!("/tapp"));
        inject_request_context_params(
            "router.state",
            &mut params,
            Some(&json!("/library")),
            None,
            None,
            None,
        );
        assert_eq!(
            params.get("currentPath").and_then(Value::as_str),
            Some("/tapp")
        );
    }

    #[test]
    fn music_and_window_snapshots_fill_status_params() {
        let mut params = HashMap::new();
        let music = json!({ "isPlaying": true, "title": "song" });
        let windows = json!({ "windows": [], "windowCount": 0 });
        inject_request_context_params(
            "music.status",
            &mut params,
            None,
            None,
            Some(&music),
            Some(&windows),
        );
        assert_eq!(params.get("status"), Some(&music));
        let mut window_params = HashMap::new();
        inject_request_context_params(
            "tapp.windows",
            &mut window_params,
            None,
            None,
            Some(&music),
            Some(&windows),
        );
        assert_eq!(window_params.get("windowState"), Some(&windows));
    }

    #[test]
    fn capabilities_without_a_declared_schema_are_unconstrained() {
        assert!(
            apply_output_contract(
                &step("router.navigate"),
                &capability("router.navigate", json!({})),
                &json!({ "whatever": true }),
            )
            .is_ok()
        );
    }

    /// 复现歌单播放链路：搜索步骤输出被整对象引用为 playlistIdFrom 时，
    /// 必须取到 playlists[0].id，而不是 message 文案
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
    fn id_param_prefers_same_name_field_then_top_level_id() {
        let output = json!({ "playlistId": "abc123", "id": "other", "message": "文案" });
        assert_eq!(
            extract_id_from_output(&output, "playlistId"),
            Some(json!("abc123"))
        );
        let output = json!({ "id": 42, "message": "文案" });
        assert_eq!(extract_id_from_output(&output, "songId"), Some(json!(42)));
        let output = json!([{ "id": "first" }, { "id": "second" }]);
        assert_eq!(
            extract_id_from_output(&output, "itemId"),
            Some(json!("first"))
        );
    }

    /// 提取不到 ID 必须返回 None（上层按未解析处理并让步骤报错），
    /// 绝不能兜底成 message 文案
    #[test]
    fn id_param_without_id_yields_none() {
        let output = json!({ "message": "找到 10 个歌单", "success": true });
        assert_eq!(extract_id_from_output(&output, "playlistId"), None);
    }
}
