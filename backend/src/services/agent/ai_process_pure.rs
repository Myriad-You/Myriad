//! Pure AI-handler param injection, prompt shaping, and image helpers.
//!
//! Handlers keep AI analyzer / HTTP / DB. This module owns:
//! - directive / steering injection into capability params
//! - systemPrompt guidance prepending
//! - semantic text extraction from step outputs
//! - prompt sanitization
//! - image dimension / prompt pure mapping

use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use std::collections::HashMap;

/// Append a steering instruction onto an existing string param.
pub fn append_instruction(params: &mut HashMap<String, Value>, key: &str, instruction: &str) {
    let existing = params.get(key).and_then(Value::as_str).unwrap_or_default();
    let combined = if existing.is_empty() {
        instruction.to_string()
    } else {
        format!(
            "{}\n\n用户最新转向指令（优先遵循）：{}",
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
        Some(sys) => format!("【补充指令 / 上下文（优先遵循）】\n{}\n\n{}", sys, prompt),
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
                    format!("用户请求：{}\n具体任务：{}", req, directive)
                } else {
                    directive.to_string()
                };
                params.insert("instruction".to_string(), Value::String(full_instruction));
            }
        }
        "ai.chat" => {
            if !params.contains_key("message") {
                let msg = if let Some(req) = user_request {
                    format!("{}（用户原始请求：{}）", directive, req)
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

/// Extract semantic text from step output JSON (avoid dumping raw arrays to the model).
pub fn extract_semantic_text(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        return s.to_string();
    }

    if let Some(obj) = value.as_object() {
        let text_keys = [
            "aiSummary",
            "analysis",
            "reply",
            "summary",
            "description",
            "message",
            "content",
        ];
        let mut parts: Vec<String> = Vec::new();

        for key in &text_keys {
            if let Some(text) = obj.get(*key).and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    parts.push(text.to_string());
                }
            }
        }

        if let Some(results) = obj.get("results").and_then(|v| v.as_array()) {
            for item in results.iter().take(10) {
                let mut item_parts: Vec<String> = Vec::new();
                for key in &["name", "title"] {
                    if let Some(v) = item
                        .get(key)
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        item_parts.push(v.to_string());
                    }
                }
                for key in &[
                    "description",
                    "snippet",
                    "status",
                    "reason",
                    "source",
                    "expectation",
                ] {
                    if let Some(v) = item
                        .get(key)
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        item_parts.push(v.to_string());
                    }
                }
                for arr_key in &["rankings", "hot_topics", "anticipated_characters"] {
                    if let Some(arr) = item.get(arr_key).and_then(|v| v.as_array()) {
                        for entry in arr.iter().take(10) {
                            let name = entry
                                .get("name")
                                .or_else(|| entry.get("character"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let desc = entry
                                .get("status")
                                .or_else(|| entry.get("reason"))
                                .or_else(|| entry.get("expectation"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let src = entry.get("source").and_then(|v| v.as_str()).unwrap_or("");
                            if !name.is_empty() {
                                if !src.is_empty() {
                                    item_parts.push(format!("{} ({}): {}", name, src, desc));
                                } else {
                                    item_parts.push(format!("{}: {}", name, desc));
                                }
                            }
                        }
                    }
                }
                if !item_parts.is_empty() {
                    parts.push(item_parts.join(" | "));
                }
            }
        }

        if !parts.is_empty() {
            return parts.join("\n\n");
        }
    }

    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// Sanitize user text for model prompts (drop control chars except newline, cap length).
pub fn sanitize_prompt_input(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(1000)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Parse image width/height: integers, whole floats, or numeric strings (`"768"` / `"768px"`).
pub fn parse_image_dim(value: &Value) -> Option<u32> {
    if let Some(n) = value.as_u64() {
        return u32::try_from(n).ok().filter(|&n| n > 0);
    }
    if let Some(n) = value.as_i64() {
        return u32::try_from(n).ok().filter(|&n| n > 0);
    }
    if let Some(n) = value.as_f64() {
        if n.is_finite() && n > 0.0 && n.fract() == 0.0 && n <= u32::MAX as f64 {
            return Some(n as u32);
        }
        return None;
    }
    if let Some(s) = value.as_str() {
        let s = s.trim();
        let s = s
            .strip_suffix("px")
            .or_else(|| s.strip_suffix("PX"))
            .unwrap_or(s)
            .trim();
        return s.parse::<u32>().ok().filter(|&n| n > 0);
    }
    None
}

/// Default image size when caller omits width/height.
pub const DEFAULT_IMAGE_WIDTH: u32 = 1024;
pub const DEFAULT_IMAGE_HEIGHT: u32 = 1024;
pub const IMAGE_DIM_MIN: u32 = 256;
pub const IMAGE_DIM_MAX: u32 = 2048;
pub const IMAGE_PROMPT_MAX_CHARS: usize = 1000;

/// Clamp a parsed image dimension into the supported range.
pub fn clamp_image_dim(dim: u32) -> u32 {
    dim.clamp(IMAGE_DIM_MIN, IMAGE_DIM_MAX)
}

/// Resolve width/height from params with defaults and clamps.
pub fn resolve_image_dimensions(params: &HashMap<String, Value>) -> (u32, u32) {
    let width = params
        .get("width")
        .and_then(parse_image_dim)
        .map(clamp_image_dim)
        .unwrap_or(DEFAULT_IMAGE_WIDTH);
    let height = params
        .get("height")
        .and_then(parse_image_dim)
        .map(clamp_image_dim)
        .unwrap_or(DEFAULT_IMAGE_HEIGHT);
    (width, height)
}

/// Extract image generation prompt from string or nested prompt.generate object.
pub fn resolve_image_prompt(params: &HashMap<String, Value>) -> Result<String, String> {
    let prompt_val = params.get("prompt");
    let prompt = prompt_val
        .and_then(|v| {
            v.as_str().map(|s| s.to_string()).or_else(|| {
                v.get("prompt")
                    .and_then(|inner| inner.as_str())
                    .map(|s| s.to_string())
            })
        })
        .ok_or_else(|| "Missing prompt parameter".to_string())?;
    if prompt.len() > IMAGE_PROMPT_MAX_CHARS {
        return Err(format!(
            "Prompt too long (max {IMAGE_PROMPT_MAX_CHARS} characters)"
        ));
    }
    Ok(prompt)
}

/// Extract negativePrompt from params or nested prompt object.
pub fn resolve_negative_prompt(params: &HashMap<String, Value>) -> Option<String> {
    params
        .get("negativePrompt")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            params
                .get("prompt")
                .and_then(|v| v.get("negativePrompt"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
}

/// Whether a capability should receive memory context injection.
pub fn capability_needs_memory(capability_id: &str) -> bool {
    matches!(
        capability_id,
        "ai.chat" | "ai.analyze" | "ai.recommend" | "compare.content" | "prompt.generate"
    )
}

/// Whether a capability should receive conversation history as `context`.
pub fn capability_needs_conversation_context(capability_id: &str) -> bool {
    matches!(
        capability_id,
        "ai.chat" | "ai.analyze" | "ai.recommend" | "compare.content"
    )
}

/// Merge role identity text into systemPrompt (pure string combine).
pub fn merge_system_prompt(existing: &str, addition: &str) -> String {
    if existing.is_empty() {
        addition.to_string()
    } else if addition.is_empty() {
        existing.to_string()
    } else {
        format!("{}\n\n{}", addition, existing)
    }
}

/// Append memory reference block onto systemPrompt.
pub fn append_memory_to_system_prompt(existing: &str, memory: &str) -> String {
    if existing.is_empty() {
        format!("参考记忆（仅供参考，不要照搬）：\n{memory}")
    } else {
        format!("{existing}\n\n参考记忆（仅供参考，不要照搬）：\n{memory}")
    }
}

/// Take the last N conversation messages (oldest-first order preserved).
pub fn take_recent_conversation_messages<T: Clone>(history: &[T], max: usize) -> Vec<T> {
    history.iter().rev().take(max).collect::<Vec<_>>().into_iter().rev().cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_image_dim_accepts_number_and_string() {
        assert_eq!(parse_image_dim(&json!(768)), Some(768));
        assert_eq!(parse_image_dim(&json!(768.0)), Some(768));
        assert_eq!(parse_image_dim(&json!("1024")), Some(1024));
        assert_eq!(parse_image_dim(&json!(" 768px ")), Some(768));
        assert_eq!(parse_image_dim(&json!(0)), None);
        assert_eq!(parse_image_dim(&json!("nope")), None);
        assert_eq!(clamp_image_dim(10), IMAGE_DIM_MIN);
        assert_eq!(clamp_image_dim(9999), IMAGE_DIM_MAX);
    }

    #[test]
    fn resolve_image_prompt_and_dims() {
        let mut params = HashMap::from([("prompt".into(), json!("a cat"))]);
        assert_eq!(resolve_image_prompt(&params).unwrap(), "a cat");
        params.insert("prompt".into(), json!({ "prompt": "nested", "negativePrompt": "blur" }));
        assert_eq!(resolve_image_prompt(&params).unwrap(), "nested");
        assert_eq!(
            resolve_negative_prompt(&params).as_deref(),
            Some("blur")
        );
        let too_long = "x".repeat(IMAGE_PROMPT_MAX_CHARS + 1);
        params.insert("prompt".into(), json!(too_long));
        assert!(resolve_image_prompt(&params).unwrap_err().contains("too long"));

        let dims = resolve_image_dimensions(&HashMap::from([
            ("width".into(), json!("512px")),
            ("height".into(), json!(768)),
        ]));
        assert_eq!(dims, (512, 768));
    }

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
    fn with_system_guidance_and_sanitize() {
        let params = HashMap::from([(
            "systemPrompt".to_string(),
            Value::String("改成简短要点".to_string()),
        )]);
        let out = with_system_guidance(&params, "原文提示".to_string());
        assert!(out.contains("改成简短要点"));
        assert!(out.contains("原文提示"));
        assert!(out.find("改成简短要点").unwrap() < out.find("原文提示").unwrap());
        assert_eq!(with_system_guidance(&HashMap::new(), "only".into()), "only");

        let dirty = "hello\x00\nworld\t";
        let clean = sanitize_prompt_input(dirty);
        assert!(!clean.contains('\0'));
        assert!(!clean.contains('\t'));
        assert!(clean.contains('\n'));
        assert_eq!(sanitize_prompt_input(&"a".repeat(2000)).len(), 1000);
    }

    #[test]
    fn semantic_text_extraction() {
        let obj = json!({
            "aiSummary": "总览",
            "results": [
                { "name": "A", "description": "d1" },
                { "title": "B", "snippet": "s2" }
            ]
        });
        let text = extract_semantic_text(&obj);
        assert!(text.contains("总览"));
        assert!(text.contains("A"));
        assert!(text.contains("B"));
        assert_eq!(extract_semantic_text(&json!("plain")), "plain");
    }

    #[test]
    fn capability_gates_and_history_window() {
        assert!(capability_needs_memory("ai.chat"));
        assert!(!capability_needs_memory("ai.image"));
        assert!(capability_needs_conversation_context("ai.analyze"));
        assert!(!capability_needs_conversation_context("speech.tts"));

        assert_eq!(
            merge_system_prompt("", "role"),
            "role"
        );
        assert!(append_memory_to_system_prompt("base", "mem").contains("参考记忆"));

        let msgs = vec![1, 2, 3, 4, 5];
        assert_eq!(take_recent_conversation_messages(&msgs, 3), vec![3, 4, 5]);
        assert_eq!(take_recent_conversation_messages(&msgs, 10), msgs);
    }
}
