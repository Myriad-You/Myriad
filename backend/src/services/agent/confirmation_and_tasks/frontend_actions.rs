// Work result assembly: final result and frontend actions.

use serde_json::{json, Value};

use super::super::agent_header::*;
use super::super::response_agent;
use super::super::types::*;

impl Agent {
    /// Successful step outputs: one success → that output; else last success (+ optional frontendActions).
    pub(crate) fn extract_final_result(&self, task_state: &TaskState) -> serde_json::Value {
        // 找到所有成功的步骤结果
        let mut results: Vec<_> = task_state
            .step_results
            .values()
            .filter(|r| r.success)
            .collect();

        results.sort_by_key(|r| &r.step_id);

        // 如果没有成功的步骤，返回失败信息
        if results.is_empty() {
            let errors: Vec<String> = task_state
                .step_results
                .values()
                .filter_map(|r| r.error.clone())
                .collect();
            let error_msg = if errors.is_empty() {
                response_agent::not_executed()
            } else {
                errors.join("; ")
            };
            return json!({
                "status": format!("{:?}", task_state.status),
                "error": error_msg
            });
        }

        // 如果只有一个结果，直接返回
        if results.len() <= 1 {
            return results
                .last()
                .and_then(|r| r.output.clone())
                .unwrap_or(json!({
                    "status": format!("{:?}", task_state.status),
                    "progress": task_state.progress
                }));
        }

        // 收集各步真正可执行的前端动作。music.control / page.interact 顶层
        // 也有字符串 `action`（"play" / "click"），不能当 frontendAction 发出去。
        let mut all_frontend_actions: Vec<Value> = Vec::new();
        for result in &results {
            if let Some(output) = &result.output {
                for action in collect_step_frontend_actions(std::iter::once(output)) {
                    tracing::info!(
                        step_id = %result.step_id,
                        action_type = ?action.get("type"),
                        "[Agent] Collected frontendAction from step"
                    );
                    all_frontend_actions.push(action);
                }
            }
        }

        // 多步骤结果：检查是否有分析/总结类型的最终结果
        let last_result = results.last().and_then(|r| r.output.as_ref());

        // 如果最后一步是分析/总结，检查是否有实际内容
        if let Some(last) = last_result {
            // 检查是否是 AI 分析结果
            if let Some((analysis, analysis_type)) = analysis_from_step_output(last) {
                // Seed `{analysis,type}`; attach search `sources` `{query,source}` (not `results`).
                let mut combined = json!({
                    "analysis": analysis,
                    "type": analysis_type
                });

                // 收集所有搜索步骤的来源信息
                let mut sources = Vec::new();
                for result in &results {
                    if let Some(output) = &result.output {
                        // 检查是否是联网搜索结果
                        if crate::services::agent::search_output::is_web_search_output(output) {
                            if let Some(query) = output.get("query").and_then(|q| q.as_str()) {
                                sources.push(json!({
                                    "query": query,
                                    "source": crate::services::agent::search_output::web_search_source_label(output)
                                }));
                            }
                        }
                        // 检查是否有 aiSummary
                        if let Some(summary) = output.get("aiSummary").and_then(|s| s.as_str()) {
                            if !summary.is_empty() && combined.get("searchSummary").is_none() {
                                combined["searchSummary"] = json!(summary);
                            }
                        }
                    }
                }

                if !sources.is_empty() {
                    combined["sources"] = json!(sources);
                }

                // 添加所有收集到的 frontendActions
                if !all_frontend_actions.is_empty() {
                    combined["frontendActions"] = json!(all_frontend_actions);
                }

                return combined;
            }

            // 检查是否是 AI 总结结果
            let inner = crate::services::agent::ai_process_pure::task_inner_value(last);
            if let Some(summary) = inner.get("summary").and_then(|s| s.as_str()) {
                if !summary.is_empty() {
                    let mut result = last.clone();
                    // 添加所有收集到的 frontendActions
                    if !all_frontend_actions.is_empty() {
                        result["frontendActions"] = json!(all_frontend_actions);
                        tracing::info!(
                            count = all_frontend_actions.len(),
                            "[Agent] Merged {} frontendActions into summary result",
                            all_frontend_actions.len()
                        );
                    }
                    return result;
                }
            }
        }

        // 默认返回最后一个结果
        let mut final_result = results
            .last()
            .and_then(|r| r.output.clone())
            .unwrap_or(json!({
                "status": format!("{:?}", task_state.status),
                "progress": task_state.progress
            }));

        // 添加所有收集到的 frontendActions
        if !all_frontend_actions.is_empty() {
            final_result["frontendActions"] = json!(all_frontend_actions);
            tracing::info!(
                count = all_frontend_actions.len(),
                "[Agent] Merged {} frontendActions into default final result",
                all_frontend_actions.len()
            );
        }

        final_result
    }

    /// 从执行结果中提取前端动作
    pub(crate) fn extract_frontend_action(&self, result: &Value) -> Option<Value> {
        extract_frontend_action_from_result(result)
    }
}

fn analysis_from_step_output(last: &Value) -> Option<(&str, &str)> {
    let inner = crate::services::agent::ai_process_pure::task_inner_value(last);
    let analysis = inner
        .get("analysis")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?;
    let ty = inner
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("general");
    Some((analysis, ty))
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
    use super::{
        analysis_from_step_output, collect_step_frontend_actions,
        extract_frontend_action_from_result,
    };
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

    #[test]
    fn analysis_from_step_output_unwraps_envelope() {
        let envelope = json!({
            "format": "json",
            "value": { "analysis": "分析正文", "type": "custom" },
            "contextProvenance": []
        });
        assert_eq!(
            analysis_from_step_output(&envelope),
            Some(("分析正文", "custom"))
        );
        assert_eq!(
            analysis_from_step_output(&json!({
                "analysis": "旧格式",
                "type": "general"
            })),
            Some(("旧格式", "general"))
        );
        assert_eq!(
            analysis_from_step_output(&json!({
                "format": "json",
                "value": { "summary": "摘要正文", "style": "brief" },
                "contextProvenance": []
            })),
            None
        );
    }
}
