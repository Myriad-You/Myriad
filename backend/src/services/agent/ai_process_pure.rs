//! Pure AI-handler param injection, prompt shaping, and image helpers.
//!
//! Handlers keep AI analyzer / HTTP / DB. This module owns:
//! - directive / steering injection into capability params
//! - systemPrompt guidance prepending
//! - semantic text extraction from step outputs
//! - prompt sanitization
//! - image dimension / prompt pure mapping

use serde_json::{json, Value};
#[cfg(test)]
use std::collections::HashMap;

pub use myriad_agent_rules::{
    append_instruction, append_memory_to_system_prompt, capability_needs_conversation_context,
    capability_needs_memory, clamp_image_dim, extract_semantic_text, inject_directive_to_params,
    inject_steering_to_params, merge_system_prompt, parse_image_dim, resolve_image_dimensions,
    resolve_image_prompt, resolve_negative_prompt, sanitize_prompt_input,
    take_recent_conversation_messages, task_inner_value, with_system_guidance,
    DEFAULT_IMAGE_HEIGHT, DEFAULT_IMAGE_WIDTH, IMAGE_DIM_MAX, IMAGE_DIM_MIN,
    IMAGE_PROMPT_MAX_CHARS, SANITIZE_PROMPT_MAX_CHARS, USER_TEXT_MAX_CHARS,
};

/// Tapp AI Task envelope used by Agent AI handlers that share that contract.
pub fn task_json_envelope(value: Value) -> Value {
    json!({
        "format": "json",
        "value": value,
        "contextProvenance": []
    })
}

pub fn task_text_envelope(value: impl Into<String>) -> Value {
    json!({
        "format": "text",
        "value": value.into(),
        "contextProvenance": []
    })
}

pub fn task_image_envelope(url: &str, width: u32, height: u32) -> Value {
    json!({
        "format": "image",
        "value": { "url": url, "width": width, "height": height },
        "contextProvenance": []
    })
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
        params.insert(
            "prompt".into(),
            json!({ "prompt": "nested", "negativePrompt": "blur" }),
        );
        assert_eq!(resolve_image_prompt(&params).unwrap(), "nested");
        assert_eq!(resolve_negative_prompt(&params).as_deref(), Some("blur"));
        let cjk = "画".repeat(400);
        assert!(cjk.len() > 1000, "regression: CJK is 3 bytes per char");
        params.insert("prompt".into(), json!(cjk));
        assert!(resolve_image_prompt(&params).is_ok());

        let too_long = "x".repeat(IMAGE_PROMPT_MAX_CHARS + 1);
        params.insert("prompt".into(), json!(too_long));
        assert!(resolve_image_prompt(&params)
            .unwrap_err()
            .contains("too long"));

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
        assert_eq!(
            sanitize_prompt_input(&"a".repeat(SANITIZE_PROMPT_MAX_CHARS + 50))
                .chars()
                .count(),
            SANITIZE_PROMPT_MAX_CHARS
        );
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

        assert_eq!(merge_system_prompt("", "role"), "role");
        assert!(append_memory_to_system_prompt("base", "mem").contains("参考记忆"));

        let msgs = vec![1, 2, 3, 4, 5];
        assert_eq!(take_recent_conversation_messages(&msgs, 3), vec![3, 4, 5]);
        assert_eq!(take_recent_conversation_messages(&msgs, 10), msgs);
    }
}
