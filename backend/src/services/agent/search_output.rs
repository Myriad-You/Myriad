//! Contract for `ai.webSearch` / `ai.groundingSearch`.
//!
//! Producers in `web_search` write [`capability_payload`]. Reply copy, display
//! hints, and analyze-step source lists recognize that object here. The
//! capability registry reuses the same input/output schemas so the two aliases
//! cannot drift.

use serde_json::{json, Value};

pub fn normalize_search_type(search_type: &str) -> &'static str {
    match search_type {
        "rss_source" => "rss_source",
        "api_docs" => "api_docs",
        _ => "general",
    }
}

pub fn is_web_search_output(value: &Value) -> bool {
    if value.get("searchType").is_some() || value.get("totalResults").is_some() {
        return true;
    }
    matches!(
        value.get("source").and_then(|v| v.as_str()),
        Some("gemini_grounding" | "google_search" | "tinyfish")
    ) && value.get("results").is_some()
}

pub fn capability_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "搜索查询内容" },
            "searchType": {
                "type": "string",
                "enum": ["rss_source", "api_docs", "general"],
                "default": "general",
                "description": "搜索类型：rss_source 搜索 RSS 源，api_docs 搜索 API 文档，general 通用搜索"
            },
            "maxResults": {
                "type": "integer",
                "default": 5,
                "description": "最大返回结果数"
            },
            "searchPrompt": {
                "type": "string",
                "description": "自定义搜索提示词"
            }
        },
        "required": ["query"]
    })
}

pub fn capability_output_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "success": { "type": "boolean" },
            "query": { "type": "string", "description": "回显的查询" },
            "searchType": { "type": "string", "description": "回显的搜索类型" },
            "aiSummary": { "type": "string", "description": "AI 对搜索结果的综述" },
            "results": { "type": "array", "description": "搜索结果列表" },
            "totalResults": { "type": "integer", "description": "结果条数" }
        }
    })
}

pub fn capability_payload(
    query: &str,
    search_type: &str,
    ai_summary: String,
    results: Vec<Value>,
) -> Value {
    json!({
        "success": true,
        "query": query,
        "searchType": search_type,
        "aiSummary": ai_summary,
        "results": results,
        "totalResults": results.len()
    })
}

pub fn web_search_source_label(output: &Value) -> Value {
    output.get("source").cloned().unwrap_or_else(|| {
        output
            .get("results")
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|item| item.get("source"))
            .cloned()
            .unwrap_or(json!("web_search"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tinyfish_payload_counts_as_web_search_output() {
        assert!(is_web_search_output(&json!({
            "searchType": "general",
            "totalResults": 0,
            "results": []
        })));
        assert!(!is_web_search_output(&json!({
            "analysis": "hello"
        })));
    }

    #[test]
    fn capability_payload_sets_search_shape() {
        let out = capability_payload("q", "general", "s".into(), vec![json!({"name": "a"})]);
        assert_eq!(out["success"], true);
        assert_eq!(out["searchType"], "general");
        assert_eq!(out["totalResults"], 1);
        assert!(is_web_search_output(&out));
        assert_eq!(web_search_source_label(&out), json!("web_search"));
    }

    #[test]
    fn source_label_prefers_result_item_source() {
        let out = json!({
            "searchType": "general",
            "results": [{"source": "tinyfish"}]
        });
        assert_eq!(web_search_source_label(&out), json!("tinyfish"));
    }

    #[test]
    fn platform_local_cache_is_not_web_search() {
        assert!(!is_web_search_output(&json!({
            "source": "local_cache",
            "wishlist": [],
            "items": [{"name": "Hades"}],
            "total": 1
        })));
        assert!(!is_web_search_output(&json!({
            "source": "tinyfish"
        })));
        assert!(is_web_search_output(&json!({
            "source": "tinyfish",
            "results": []
        })));
    }

    #[test]
    fn payload_keys_match_declared_output_schema() {
        let out = capability_payload("q", "general", "s".into(), vec![json!({"name": "a"})]);
        let schema = capability_output_schema();
        let declared: Vec<String> = schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|props| props.keys().cloned().collect())
            .expect("schema properties");
        let produced = out.as_object().expect("payload object");
        for key in &declared {
            assert!(produced.contains_key(key), "payload missing declared {key}");
        }
        for key in produced.keys() {
            assert!(
                declared.iter().any(|declared| declared == key),
                "payload has undeclared {key}"
            );
        }
    }

    #[test]
    fn normalize_search_type_clamps_to_declared_enum() {
        assert_eq!(normalize_search_type("rss_source"), "rss_source");
        assert_eq!(normalize_search_type("api_docs"), "api_docs");
        assert_eq!(normalize_search_type("general"), "general");
        assert_eq!(normalize_search_type("nope"), "general");
        assert_eq!(normalize_search_type(""), "general");
    }

    #[test]
    fn input_schema_drops_unused_result_format_and_source() {
        let props = capability_input_schema()
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .expect("schema properties");
        assert!(props.contains_key("query"));
        assert!(props.contains_key("searchType"));
        assert!(props.contains_key("maxResults"));
        assert!(props.contains_key("searchPrompt"));
        assert!(!props.contains_key("resultFormat"));
        assert!(!props.contains_key("source"));
    }
}
