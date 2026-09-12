// Executor path and AI helper methods

use crate::config::ModelTier;
use crate::services::agent::capability::get_registry;
use crate::services::agent::types::*;
use myriad_agent_rules::{extract_json_array_from_ai_response, untrusted_block};
use serde_json::Value;
use std::collections::HashMap;

use super::summarize_output;
use super::Executor;

fn walk_dot_path(root: &Value, parts: &[&str]) -> Option<Value> {
    let mut current = root.clone();
    for part in parts {
        current = if let Ok(idx) = part.parse::<usize>() {
            current.as_array().and_then(|arr| arr.get(idx).cloned())?
        } else {
            current.get(*part).cloned()?
        };
    }
    Some(current)
}

impl Executor {
    /// 解析 dot-path 从步骤输出中取值
    pub(crate) fn resolve_dot_path(
        &self,
        path: &str,
        _current_step_id: &str,
        current_output: &Value,
        context: &ExecutionContext,
    ) -> Value {
        let parts: Vec<&str> = path.split('.').collect();
        if parts.is_empty() {
            return Value::Null;
        }

        // 确定起始值
        let (root, field_start) = if parts[0] == "output" {
            // "output.xxx" → 当前步骤输出
            (current_output, 1)
        } else if context.step_outputs.contains_key(parts[0]) {
            // "step_id.xxx" → 指定步骤输出
            match context.step_outputs.get(parts[0]) {
                Some(v) => (v, 1),
                None => return Value::Null,
            }
        } else {
            // 没有前缀，尝试从当前步骤输出解析
            (current_output, 0)
        };

        walk_dot_path(root, &parts[field_start..]).unwrap_or_else(|| {
            walk_dot_path(
                crate::services::agent::ai_process_pure::task_inner_value(root),
                &parts[field_start..],
            )
            .unwrap_or(Value::Null)
        })
    }

    /// 判断 JSON 值的真值
    pub(crate) fn is_truthy(value: &Value) -> bool {
        match value {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_f64().is_some_and(|v| v != 0.0),
            Value::String(s) => !s.is_empty(),
            Value::Array(arr) => !arr.is_empty(),
            Value::Object(obj) => !obj.is_empty(),
        }
    }

    /// AI 动态生成步骤
    pub(crate) async fn ai_generate_steps(
        &self,
        context_prompt: &str,
        capability_scope: Option<&[String]>,
        parent_step: &RecipeStep,
        context: &ExecutionContext,
    ) -> Vec<RecipeStep> {
        let analyzer = match self.get_analyzer_for_tier(ModelTier::Standard) {
            Some(a) => a,
            None => {
                tracing::warn!("[Generator] No AI analyzer available for AiGenerated");
                return vec![];
            }
        };

        // 构建能力列表
        let capabilities = if let Some(scope) = capability_scope {
            scope.join(", ")
        } else {
            let registry = get_registry().await;
            registry
                .get_all()
                .iter()
                .take(30)
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };

        // 构建上下文摘要（按 step id 排序，保证 AI 每次看到一致的上下文顺序）
        // 这些输出里有 ai.webSearch / web.scrape / brew.article 抓回来的正文，
        // 是别人能写的内容；而这个提示词的产物是**要被执行的步骤**。所以必须
        // 划边界，否则正文里一句「忽略以上」就直通执行层。
        let outputs_summary: String = {
            let mut pairs: Vec<_> = context.step_outputs.iter().collect();
            pairs.sort_by_key(|(id, _)| *id);
            let body = pairs
                .iter()
                .take(5)
                .map(|(id, val)| format!("- {}: {}", id, summarize_output(val).unwrap_or_default()))
                .collect::<Vec<_>>()
                .join("\n");
            untrusted_block("step_output", &body)
        };

        let prompt = format!(
            r#"Given the context below, generate the next steps to run.

## Original user request (highest priority; every step must serve this)
{user_request}

## Context notes
{ctx}

## Outputs from finished steps
{outputs}

## Available capabilities
{caps}

## Rules
1. Each step's action must say what that step does, and it must relate to the original request
2. Do not emit steps unrelated to the request
3. Emit at most 3 steps
4. Output a JSON array only, no other text

## Output format
```json
[{{"id": "gen_1", "capability_id": "...", "action": "what this step does", "params": {{}}}}]
```"#,
            user_request = context.original_request,
            ctx = context_prompt,
            outputs = outputs_summary,
            caps = capabilities
        );

        let result = analyzer.analyze(&prompt).await;
        match result {
            Ok(response) => {
                // 数组提取走共享实现：它带区间校验，也认 ```json 围栏；
                // 解析不出来时返回空数组，等价于「这一轮不追加步骤」。
                let items = extract_json_array_from_ai_response(response.trim());
                if items.is_empty() {
                    tracing::warn!(
                        response_preview = %response.chars().take(200).collect::<String>(),
                        "[Generator] AI-generated steps were unparseable or empty"
                    );
                }
                items
                    .into_iter()
                    .take(3)
                    .enumerate()
                    .filter_map(|(i, item)| {
                        let id = item
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let cap_id = item
                            .get("capability_id")
                            .and_then(|v| v.as_str())?
                            .to_string();
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
                            timeout_ms: Some(300_000),
                            model_tier: None,
                            generator: None,
                        })
                    })
                    .collect()
            }
            Err(e) => {
                tracing::warn!(error = %e, "[Generator] AI call failed");
                vec![]
            }
        }
    }
}
