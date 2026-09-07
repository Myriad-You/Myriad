//! Semantic text extraction and capability context switches. No I/O.

use serde_json::Value;

/// Inner payload of a Tapp AI Task envelope, or the value itself if unwrapped.
///
/// Envelope shape: `{ format, value, contextProvenance }`. Agent AI handlers
/// that share the Tapp contract wrap their payload this way; consumers that
/// want `summary` / `analysis` / `url` must look inside `value`.
pub fn task_inner_value(output: &Value) -> &Value {
    match output {
        Value::Object(map)
            if map.get("format").and_then(Value::as_str).is_some() && map.contains_key("value") =>
        {
            &map["value"]
        }
        other => other,
    }
}

/// Extract semantic text from step output JSON (avoid dumping raw arrays to the model).
pub fn extract_semantic_text(value: &Value) -> String {
    let value = task_inner_value(value);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn semantic_text_unwraps_task_envelope() {
        let envelope = json!({
            "format": "json",
            "value": { "analysis": "分析正文", "type": "custom" },
            "contextProvenance": []
        });
        assert_eq!(extract_semantic_text(&envelope), "分析正文");
        assert_eq!(
            extract_semantic_text(&json!({
                "format": "text",
                "value": "回复正文",
                "contextProvenance": []
            })),
            "回复正文"
        );
        assert_eq!(
            task_inner_value(&envelope)
                .get("analysis")
                .and_then(Value::as_str),
            Some("分析正文")
        );
    }

    #[test]
    fn capability_gates() {
        assert!(capability_needs_memory("ai.chat"));
        assert!(!capability_needs_memory("ai.image"));
        assert!(capability_needs_conversation_context("ai.analyze"));
        assert!(!capability_needs_conversation_context("speech.tts"));
    }
}
