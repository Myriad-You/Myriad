//! Steering / directive injection into capability params. No I/O.

use serde_json::Value;
use std::collections::HashMap;

/// Append a steering instruction onto an existing string param.
pub fn append_instruction(params: &mut HashMap<String, Value>, key: &str, instruction: &str) {
    let existing = params.get(key).and_then(Value::as_str).unwrap_or_default();
    let combined = if existing.is_empty() {
        instruction.to_string()
    } else {
        format!(
            "{}\n\nLatest user steering (follow this first): {}",
            existing, instruction
        )
    };
    params.insert(key.to_string(), Value::String(combined));
}

/// Prepend systemPrompt (role/memory/steer fallback) to a freeform model prompt.
///
/// Handlers that already consume systemPrompt as a first-class channel (e.g. ai.chat)
/// should not call this to avoid double-application.
pub fn with_system_guidance(params: &HashMap<String, Value>, prompt: String) -> String {
    match params
        .get("systemPrompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(sys) => format!(
            "Additional instructions / context (follow this first):\n{}\n\n{}",
            sys, prompt
        ),
        None => prompt,
    }
}

/// Steering is newer than the Planner output, so it must augment existing
/// parameters rather than only filling empty fields.
///
/// Prefer the field each handler already reads. Always also leave a trail on
/// `systemPrompt` so handlers that only build freeform prompts still honor
/// steer via [`with_system_guidance`].
pub fn inject_steering_to_params(
    capability_id: &str,
    instruction: &str,
    params: &mut HashMap<String, Value>,
) {
    // Shared channel: consumed by ai.chat natively and by with_system_guidance.
    append_instruction(params, "systemPrompt", instruction);

    match capability_id {
        "ai.summarize" => append_instruction(params, "focus", instruction),
        "ai.analyze" | "compare.content" => append_instruction(params, "instruction", instruction),
        "ai.chat" => append_instruction(params, "message", instruction),
        "ai.webSearch" | "ai.groundingSearch" => append_instruction(params, "query", instruction),
        "prompt.generate" => append_instruction(params, "description", instruction),
        "ai.image" => append_instruction(params, "prompt", instruction),
        "translate.text" | "code.explain" | "ai.recommend" | "smart.filter"
        | "brewlia.annotate" | "brewlia.podcast" => {
            // systemPrompt trail + with_system_guidance in the handler is enough.
        }
        _ => {}
    }
}

/// Inject Planner directive into capability-specific params (fill empty fields only).
pub fn inject_directive_to_params(
    capability_id: &str,
    directive: &str,
    user_request: Option<&str>,
    params: &mut HashMap<String, Value>,
) {
    if directive.is_empty() {
        return;
    }

    match capability_id {
        "ai.analyze" | "compare.content" => {
            if !params.contains_key("instruction") {
                let full_instruction = if let Some(req) = user_request {
                    format!("User request: {}\nTask: {}", req, directive)
                } else {
                    directive.to_string()
                };
                params.insert("instruction".to_string(), Value::String(full_instruction));
            }
        }
        "ai.chat" => {
            if !params.contains_key("message") {
                let msg = if let Some(req) = user_request {
                    format!("{} (original user request: {})", directive, req)
                } else {
                    directive.to_string()
                };
                params.insert("message".to_string(), Value::String(msg));
            }
        }
        "ai.summarize" => {
            if !params.contains_key("focus") {
                params.insert("focus".to_string(), Value::String(directive.to_string()));
            }
        }
        "prompt.generate" => {
            let has_content = params
                .get("title")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty())
                || params
                    .get("description")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.is_empty())
                || params
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.is_empty());
            if !has_content {
                params.insert(
                    "description".to_string(),
                    Value::String(directive.to_string()),
                );
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn steering_and_directive_injection() {
        let mut params = HashMap::from([(
            "instruction".to_string(),
            Value::String("旧计划".to_string()),
        )]);
        inject_steering_to_params("ai.analyze", "只看最近数据", &mut params);
        let instruction = params["instruction"].as_str().unwrap();
        assert!(instruction.contains("旧计划"));
        assert!(instruction.contains("只看最近数据"));
        assert!(params["systemPrompt"]
            .as_str()
            .unwrap()
            .contains("只看最近数据"));

        let mut params = HashMap::new();
        inject_steering_to_params("ai.webSearch", "改查官方文档", &mut params);
        assert_eq!(params["query"], json!("改查官方文档"));

        let mut params = HashMap::new();
        inject_directive_to_params("ai.chat", "请总结", Some("用户问天气"), &mut params);
        assert!(params["message"].as_str().unwrap().contains("请总结"));
        assert!(params["message"].as_str().unwrap().contains("天气"));

        let mut params = HashMap::from([("instruction".into(), json!("keep"))]);
        inject_directive_to_params("ai.analyze", "ignored", None, &mut params);
        assert_eq!(params["instruction"], json!("keep"));
    }

    #[test]
    fn with_system_guidance_orders_steer_before_prompt() {
        let params = HashMap::from([(
            "systemPrompt".to_string(),
            Value::String("改成简短要点".to_string()),
        )]);
        let out = with_system_guidance(&params, "原文提示".to_string());
        assert!(out.contains("改成简短要点"));
        assert!(out.contains("原文提示"));
        assert!(out.find("改成简短要点").unwrap() < out.find("原文提示").unwrap());
        assert_eq!(with_system_guidance(&HashMap::new(), "only".into()), "only");
    }
}
