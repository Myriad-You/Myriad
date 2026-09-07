//! Pure helpers for agent external (HTTP / MCP / scrape) handlers.
//!
//! Handlers keep outbound HTTP. Domain symbols live in `myriad-agent-rules`
//! and are re-exported here so existing imports compile.

use serde_json::{json, Value};
use std::collections::HashMap;

pub use myriad_agent_rules::{
    classify_outbound_fetch, compress_and_truncate_text, first_i64_param, first_string_param,
    hitokoto_type, http_body_exceeds_limit, http_body_size_error, http_content_length_error,
    http_fetch_method, match_mcp_capability_id, mcp_arguments, optional_string_param,
    parse_http_body_value, parse_mcp_capability_id, sanitize_http_headers, scrape_max_length,
    scrape_selector, scrape_should_skip_tag, HTTP_FETCH_MAX_BODY_BYTES, SCRAPE_SKIP_TAGS,
    WEB_SCRAPE_DEFAULT_MAX_LENGTH, WEB_SCRAPE_MAX_HTML_BYTES,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_string_and_http_helpers() {
        let mut params = HashMap::new();
        params.insert("username".into(), json!("  ada  "));
        params.insert("empty".into(), json!("  "));
        assert_eq!(
            optional_string_param(&params, "username").as_deref(),
            Some("ada")
        );
        assert!(optional_string_param(&params, "empty").is_none());
        params.insert("steamId".into(), json!("765"));
        params.insert("uid".into(), json!(42));
        assert_eq!(
            first_string_param(&params, &["steamId", "steam_id"]).as_deref(),
            Some("765")
        );
        assert_eq!(first_i64_param(&params, &["uid"]), Some(42));

        params.insert("method".into(), json!("POST"));
        assert_eq!(http_fetch_method(&params), "POST");
        assert_eq!(http_fetch_method(&HashMap::new()), "GET");

        assert!(!http_body_exceeds_limit(100));
        assert!(http_body_exceeds_limit(HTTP_FETCH_MAX_BODY_BYTES + 1));
        assert!(http_content_length_error(99).contains("99"));
        assert_eq!(parse_http_body_value(r#"{"a":1}"#)["a"], 1);
        assert_eq!(parse_http_body_value("not-json"), json!("not-json"));
    }

    #[test]
    fn mcp_capability_and_arguments() {
        assert_eq!(
            parse_mcp_capability_id("mcp.github.search").unwrap(),
            ("github", "search")
        );
        assert!(parse_mcp_capability_id("github.search").is_err());
        assert!(parse_mcp_capability_id("mcp.only").is_err());
        let advertised = vec![("com.example".to_string(), "search".to_string())];
        assert_eq!(
            match_mcp_capability_id("mcp.com.example.search", &advertised).unwrap(),
            ("com.example".to_string(), "search".to_string())
        );

        let params = HashMap::from([
            ("query".to_string(), json!("myriad")),
            ("__directive".to_string(), json!("internal")),
            ("__user_request".to_string(), json!("private")),
            ("__steering".to_string(), json!("new direction")),
        ]);
        assert_eq!(mcp_arguments(&params), json!({ "query": "myriad" }));
    }

    #[test]
    fn scrape_helpers() {
        let mut params = HashMap::new();
        params.insert("selector".into(), json!("article"));
        params.insert("max_length".into(), json!(10));
        assert_eq!(scrape_selector(&params), "article");
        assert_eq!(scrape_max_length(&params), 10);
        assert_eq!(scrape_selector(&HashMap::new()), "body");
        assert_eq!(
            scrape_max_length(&HashMap::new()),
            WEB_SCRAPE_DEFAULT_MAX_LENGTH
        );

        let (text, truncated) = compress_and_truncate_text("  a   b  c  d  e  ", 5);
        assert!(truncated);
        assert_eq!(text.chars().count(), 5);
        assert!(scrape_should_skip_tag("script"));
        assert!(!scrape_should_skip_tag("p"));
    }

    #[test]
    fn outbound_fetch_keeps_status_and_drops_reqwest() {
        assert_eq!(
            classify_outbound_fetch(
                "Failed to fetch Bangumi user",
                "Bangumi API error: 404 Not Found"
            ),
            "Failed to fetch Bangumi user (HTTP 404)"
        );
        assert_eq!(
            classify_outbound_fetch(
                "Failed to fetch Bilibili user",
                "error sending request for url (https://api.bilibili.com/x/space/acc/info)"
            ),
            "Failed to fetch Bilibili user"
        );
        assert_eq!(
            classify_outbound_fetch("Failed to fetch weather", "timed out"),
            "Failed to fetch weather: timed out"
        );
        assert_eq!(
            classify_outbound_fetch("Failed to fetch Bilibili user", "用户不存在"),
            "Failed to fetch Bilibili user: 用户不存在"
        );
        assert_eq!(
            classify_outbound_fetch(
                "Failed to fetch hitokoto",
                "expected value at line 1 column 1"
            ),
            "Failed to fetch hitokoto"
        );
    }
}
