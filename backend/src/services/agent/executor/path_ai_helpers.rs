// Executor path and AI helper methods

use crate::config::ModelTier;
use crate::services::agent::capability::get_registry;
use crate::services::agent::types::*;
use serde_json::Value;
use std::collections::HashMap;

use super::Executor;
use super::summarize_output;

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

        // 逐层取值
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
        let outputs_summary: String = {
            let mut pairs: Vec<_> = context.step_outputs.iter().collect();
            pairs.sort_by_key(|(id, _)| *id);
            pairs
                .iter()
                .take(5)
                .map(|(id, val)| format!("- {}: {}", id, summarize_output(val).unwrap_or_default()))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let prompt = format!(
            r#"根据以下上下文，生成接下来需要执行的步骤。

## 用户原始请求（最高优先级，所有步骤都必须服务于此目标）
{user_request}

## 上下文提示
{ctx}

## 已完成步骤的输出
{outputs}

## 可用能力
{caps}

## 重要规则
1. 每个步骤的 action 字段必须明确写出该步骤要做什么，且必须与用户原始请求直接相关
2. 不要生成与用户请求无关的步骤
3. 最多生成 3 个步骤
4. 只输出 JSON 数组，不要其他文字

## 输出格式
```json
[{{"id": "gen_1", "capability_id": "...", "action": "具体说明这步做什么", "params": {{}}}}]
```"#,
            user_request = context.original_request,
            ctx = context_prompt,
            outputs = outputs_summary,
            caps = capabilities
        );

        let result = analyzer.analyze(&prompt).await;
        match result {
            Ok(response) => {
                let text = response.trim();
                // 提取 JSON 数组
                let json_str = if let Some(start) = text.find('[') {
                    if let Some(end) = text.rfind(']') {
                        &text[start..=end]
                    } else {
                        text
                    }
                } else {
                    text
                };

                match serde_json::from_str::<Vec<Value>>(json_str) {
                    Ok(items) => items
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
                                timeout_ms: Some(30_000),
                                model_tier: None,
                                generator: None,
                            })
                        })
                        .collect(),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "[Generator] Failed to parse AI-generated steps"
                        );
                        vec![]
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "[Generator] AI call failed");
                vec![]
            }
        }
    }
}
