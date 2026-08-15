//! Pure helpers for agent external (HTTP / MCP / scrape) handlers.
//!
//! Handlers keep outbound HTTP. Domain owns:
//! - optional string param projection
//! - MCP capability id + argument filtering
//! - HTTP method / body size gates
//! - scrape length clamp and text compression
//! - simple HTML title extraction

use serde_json::{json, Value};
use std::collections::HashMap;

/// Max response body accepted by http.fetch (bytes).
pub const HTTP_FETCH_MAX_BODY_BYTES: u64 = 10 * 1024 * 1024;
/// Max HTML size for web.scrape (bytes).
pub const WEB_SCRAPE_MAX_HTML_BYTES: usize = 5 * 1024 * 1024;
/// Default max extracted text length for web.scrape.
pub const WEB_SCRAPE_DEFAULT_MAX_LENGTH: usize = 5000;

/// Non-empty trimmed string from params.
pub fn optional_string_param(params: &HashMap<String, Value>, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// HTTP method for http.fetch (default GET; only POST is special-cased).
pub fn http_fetch_method(params: &HashMap<String, Value>) -> &str {
    params
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET")
}

/// Whether Content-Length / body exceeds http.fetch limit.
pub fn http_body_exceeds_limit(len: u64) -> bool {
    len > HTTP_FETCH_MAX_BODY_BYTES
}

/// User-facing error when body is too large (Content-Length path).
pub fn http_content_length_error(content_length: u64) -> String {
    format!("Response Content-Length ({content_length} bytes) exceeds 10MB limit")
}

/// User-facing error when body is too large (after read).
pub fn http_body_size_error() -> String {
    "Response body exceeds 10MB limit".to_string()
}

/// Parse response body as JSON or wrap as string Value.
pub fn parse_http_body_value(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|_| json!(body))
}

// ── MCP ─────────────────────────────────────────────────────────────────────

/// Parse `mcp.{server_id}.{tool_name}` capability id.
pub fn parse_mcp_capability_id(capability_id: &str) -> Result<(&str, &str), String> {
    let rest = capability_id
        .strip_prefix("mcp.")
        .ok_or_else(|| "Invalid MCP capability ID".to_string())?;
    rest.split_once('.')
        .ok_or_else(|| "MCP capability ID must include server and tool names".to_string())
}

/// Strip executor-only `__*` keys before crossing the MCP trust boundary.
pub fn mcp_arguments(params: &HashMap<String, Value>) -> Value {
    Value::Object(
        params
            .iter()
            .filter(|(key, _)| !key.starts_with("__"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

// ── Web scrape ──────────────────────────────────────────────────────────────

/// CSS selector for scrape (default body).
pub fn scrape_selector(params: &HashMap<String, Value>) -> &str {
    params
        .get("selector")
        .and_then(|v| v.as_str())
        .unwrap_or("body")
}

/// Max text length for scrape (default 5000).
pub fn scrape_max_length(params: &HashMap<String, Value>) -> usize {
    params
        .get("max_length")
        .and_then(|v| v.as_u64())
        .unwrap_or(WEB_SCRAPE_DEFAULT_MAX_LENGTH as u64) as usize
}

/// Whether raw HTML exceeds scrape size gate.
pub fn scrape_html_too_large(html_len: usize) -> bool {
    html_len > WEB_SCRAPE_MAX_HTML_BYTES
}

/// Collapse whitespace and truncate to max_length; returns (text, truncated).
pub fn compress_and_truncate_text(text: &str, max_length: usize) -> (String, bool) {
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated = text.chars().count() > max_length;
    let text: String = text.chars().take(max_length).collect();
    (text, truncated)
}

/// Tags whose text nodes should be skipped during scrape.
pub const SCRAPE_SKIP_TAGS: &[&str] = &["script", "style", "noscript", "svg", "iframe"];

pub fn scrape_should_skip_tag(tag: &str) -> bool {
    SCRAPE_SKIP_TAGS.contains(&tag)
}

// ── Shared tiny helpers ─────────────────────────────────────────────────────

/// Hitokoto type param (optional).
pub fn hitokoto_type(params: &HashMap<String, Value>) -> Option<&str> {
    params.get("type").and_then(|v| v.as_str())
}

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

        assert!(!scrape_html_too_large(100));
        assert!(scrape_html_too_large(WEB_SCRAPE_MAX_HTML_BYTES + 1));

        let (text, truncated) = compress_and_truncate_text("  a   b  c  d  e  ", 5);
        assert!(truncated);
        assert_eq!(text.chars().count(), 5);
        assert!(scrape_should_skip_tag("script"));
        assert!(!scrape_should_skip_tag("p"));
    }
}
