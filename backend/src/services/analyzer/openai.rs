use anyhow::{Context, Result};
use std::future::Future;

use super::types::{json_token, OpenAIResponse, StreamDelta};

/// Format a failed OpenAI-compatible HTTP response for user-facing errors.
///
/// The transport is OpenAI-compatible (OpenRouter / DeepSeek / custom gateways),
/// not necessarily official OpenAI — keep the wording accurate so region blocks
/// and wrong models are not misread as "using OpenAI".
pub(super) fn format_openai_compatible_http_error(
    status: reqwest::StatusCode,
    endpoint: &str,
    model: &str,
    body: &str,
) -> String {
    let body = body.trim();
    let region_blocked = body
        .to_ascii_lowercase()
        .contains("not available in your region")
        || body.contains("\"code\":403")
        || body.contains("\"code\": 403");

    let mut msg = format!(
        "OpenAI-compatible API error {status} (endpoint: {endpoint}, model: {model}): {body}"
    );

    if region_blocked {
        msg.push_str(
            " — this is a provider geo-restriction on the server egress IP (common with OpenRouter for Claude / Grok / GPT / Gemini), not a wrong provider switch. Switch to a region-available model, or enable an outbound proxy in settings.",
        );
    }

    msg
}

/// Extract assistant text from an OpenAI-compatible chat completion body.
///
/// Never panics: returns clear anyhow errors for playground 502 paths.
pub(super) fn extract_openai_completion_text(response: &OpenAIResponse) -> Result<String> {
    if let Some(error) = &response.error {
        let message = error
            .message
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("OpenAI-compatible provider returned an error object");
        let code = error
            .code
            .as_ref()
            .map(|c| match c {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .filter(|s| !s.is_empty() && s != "null");
        return match code {
            Some(code) => Err(anyhow::anyhow!(
                "OpenAI-compatible API error (code {code}): {message}"
            )),
            None => Err(anyhow::anyhow!("OpenAI-compatible API error: {message}")),
        };
    }

    let message = response
        .choices
        .first()
        .map(|choice| &choice.message)
        .ok_or_else(|| anyhow::anyhow!("OpenAI-compatible API returned no choices"))?;

    if let Some(content) = message
        .content
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return Ok(content.to_string());
    }

    // Prefer non-empty content; fall back to common reasoning fields when
    // providers put the payload only in reasoning_* (still never panic).
    for candidate in [
        message.reasoning_content.as_deref(),
        message.reasoning.as_deref(),
    ] {
        if let Some(text) = candidate.map(str::trim).filter(|s| !s.is_empty()) {
            return Ok(text.to_string());
        }
    }

    Err(anyhow::anyhow!(
        "OpenAI-compatible API returned empty message content"
    ))
}

/// OpenAI-compatible chat.completion.chunk → text / reasoning deltas.
///
/// Grok / DeepSeek / OpenRouter put the thinking trace on `delta.reasoning_content`
/// (sometimes `delta.reasoning`). The visible answer stays on `delta.content`.
fn reasoning_text_from_value(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = json_token(value) {
        return Some(text.to_string());
    }
    if let Some(obj) = value.as_object() {
        if let Some(text) = json_token(&obj["content"]).or_else(|| json_token(&obj["text"])) {
            return Some(text.to_string());
        }
    }
    None
}

pub fn openai_stream_deltas(json: &serde_json::Value) -> Vec<StreamDelta> {
    if let Some(kind) = json.get("type").and_then(|v| v.as_str()) {
        if kind == "response.reasoning_text.delta"
            || kind == "response.reasoning_summary_text.delta"
        {
            if let Some(text) = reasoning_text_from_value(&json["delta"]) {
                return vec![StreamDelta::Reasoning(text)];
            }
        }
        if kind == "response.output_text.delta" {
            if let Some(text) = json_token(&json["delta"]) {
                return vec![StreamDelta::Text(text.to_string())];
            }
        }
    }

    let Some(delta) = json.pointer("/choices/0/delta") else {
        return Vec::new();
    };
    let mut out = Vec::new();

    if let Some(text) = reasoning_text_from_value(&delta["reasoning_content"])
        .or_else(|| reasoning_text_from_value(&delta["reasoning"]))
    {
        out.push(StreamDelta::Reasoning(text));
    } else if let Some(details) = delta.get("reasoning_details").and_then(|v| v.as_array()) {
        let mut joined = String::new();
        for item in details {
            if let Some(text) = json_token(&item["text"]).or_else(|| json_token(&item["content"])) {
                joined.push_str(text);
            }
        }
        if !joined.is_empty() {
            out.push(StreamDelta::Reasoning(joined));
        }
    }

    if let Some(text) = json_token(&delta["content"]) {
        out.push(StreamDelta::Text(text.to_string()));
    }
    out
}

pub(super) async fn consume_openai_sse<F, Fut>(
    mut response: reqwest::Response,
    mut on_delta: F,
) -> Result<String>
where
    F: FnMut(StreamDelta) -> Fut + Send,
    Fut: Future<Output = bool> + Send,
{
    let mut full_text = String::new();
    let mut buffer = String::new();
    loop {
        let chunk = response.chunk().await.context("Stream read error")?;
        match chunk {
            Some(bytes) => buffer.push_str(&String::from_utf8_lossy(&bytes)),
            None => break,
        }
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer = buffer[pos + 1..].to_string();
            if line.is_empty() {
                continue;
            }
            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };
            if data.trim() == "[DONE]" {
                return Ok(full_text);
            }
            let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            for delta in openai_stream_deltas(&json) {
                if let StreamDelta::Text(ref content) = delta {
                    full_text.push_str(content);
                }
                if !on_delta(delta).await {
                    return Ok(full_text);
                }
                tokio::task::yield_now().await;
            }
        }
    }
    Ok(full_text)
}

pub(crate) fn openai_chat_completions_url(base_url: Option<&str>) -> String {
    let base_url = base_url
        .unwrap_or("https://api.openai.com/v1")
        .trim()
        .trim_end_matches('/');

    if base_url.ends_with("/chat/completions") {
        return base_url.to_string();
    }

    // OpenRouter documents its OpenAI-compatible API under /api/v1. Accepting
    // the site root here makes a common settings mistake safe without changing
    // the semantics of custom OpenAI-compatible endpoints.
    if matches!(base_url, "https://openrouter.ai" | "http://openrouter.ai") {
        return format!("{base_url}/api/v1/chat/completions");
    }

    // The official OpenAI host is the other common root-only value.
    if matches!(base_url, "https://api.openai.com" | "http://api.openai.com") {
        return format!("{base_url}/v1/chat/completions");
    }

    format!("{base_url}/chat/completions")
}

#[cfg(test)]
mod tests {
    use super::super::types::{OpenAIResponse, StreamDelta};
    use super::{
        extract_openai_completion_text, format_openai_compatible_http_error,
        openai_chat_completions_url, openai_stream_deltas,
    };
    use serde_json::json;

    #[test]
    fn openai_compatible_error_does_not_claim_official_openai() {
        let msg = format_openai_compatible_http_error(
            reqwest::StatusCode::FORBIDDEN,
            "https://openrouter.ai/api/v1/chat/completions",
            "x-ai/grok-4.5",
            r#"{"error":{"message":"This model is not available in your region.","code":403}}"#,
        );
        assert!(msg.contains("OpenAI-compatible API error"));
        assert!(!msg.starts_with("OpenAI API error"));
        assert!(msg.contains("x-ai/grok-4.5"));
        assert!(msg.contains("openrouter.ai"));
        assert!(msg.contains("geo-restriction"));
    }

    #[test]
    fn normalizes_openai_compatible_chat_urls() {
        assert_eq!(
            openai_chat_completions_url(None),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url(Some("https://api.openai.com")),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url(Some("https://openrouter.ai/")),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url(Some("https://openrouter.ai/api/v1")),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url(Some("https://gateway.example.com/v1/chat/completions")),
            "https://gateway.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn openai_response_deserializes_null_content() {
        let raw = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "reasoning_content": "step by step: final answer HERE"
                }
            }]
        }"#;
        let parsed: OpenAIResponse = serde_json::from_str(raw).expect("deserialize");
        let text = extract_openai_completion_text(&parsed).expect("fallback to reasoning");
        assert!(text.contains("final answer HERE"));
    }

    #[test]
    fn openai_response_top_level_error_is_err() {
        let raw = r#"{
            "error": {
                "message": "Provider returned error",
                "code": "model_not_found"
            },
            "choices": []
        }"#;
        let parsed: OpenAIResponse = serde_json::from_str(raw).expect("deserialize");
        let err = extract_openai_completion_text(&parsed).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("Provider returned error"));
        assert!(msg.contains("model_not_found"));
    }

    #[test]
    fn openai_response_empty_content_without_reasoning_is_err() {
        let raw = r#"{
            "choices": [{
                "message": { "role": "assistant", "content": null }
            }]
        }"#;
        let parsed: OpenAIResponse = serde_json::from_str(raw).expect("deserialize");
        let err = extract_openai_completion_text(&parsed).unwrap_err();
        assert!(format!("{err:#}").contains("empty message content"));
    }

    #[test]
    fn openai_response_prefers_non_empty_content() {
        let raw = r#"{
            "choices": [{
                "message": {
                    "content": "  visible payload  ",
                    "reasoning_content": "hidden chain"
                }
            }]
        }"#;
        let parsed: OpenAIResponse = serde_json::from_str(raw).expect("deserialize");
        let text = extract_openai_completion_text(&parsed).expect("content");
        assert_eq!(text, "visible payload");
    }

    #[test]
    fn openai_stream_reads_reasoning_content_separately_from_answer() {
        let chunk = json!({
            "choices": [{
                "delta": {
                    "reasoning_content": "let me count",
                    "content": "4"
                }
            }]
        });
        assert_eq!(
            openai_stream_deltas(&chunk),
            vec![
                StreamDelta::Reasoning("let me count".to_string()),
                StreamDelta::Text("4".to_string()),
            ]
        );
    }

    #[test]
    fn openai_stream_reads_reasoning_object_and_responses_events() {
        let object = json!({
            "choices": [{ "delta": { "reasoning": { "content": "energy first" } } }]
        });
        assert_eq!(
            openai_stream_deltas(&object),
            vec![StreamDelta::Reasoning("energy first".to_string())]
        );

        let responses = json!({
            "type": "response.reasoning_summary_text.delta",
            "delta": "then impact speed"
        });
        assert_eq!(
            openai_stream_deltas(&responses),
            vec![StreamDelta::Reasoning("then impact speed".to_string())]
        );
    }

    #[test]
    fn openai_stream_reads_reasoning_alias_and_skips_empty() {
        let reasoning_only = json!({
            "choices": [{ "delta": { "reasoning": "scratch" } }]
        });
        assert_eq!(
            openai_stream_deltas(&reasoning_only),
            vec![StreamDelta::Reasoning("scratch".to_string())]
        );

        let empty = json!({ "choices": [{ "delta": { "content": "" } }] });
        assert!(openai_stream_deltas(&empty).is_empty());
    }

    #[test]
    fn openai_stream_joins_reasoning_details() {
        let chunk = json!({
            "choices": [{
                "delta": {
                    "reasoning_details": [
                        { "text": "step " },
                        { "content": "two" }
                    ]
                }
            }]
        });
        assert_eq!(
            openai_stream_deltas(&chunk),
            vec![StreamDelta::Reasoning("step two".to_string())]
        );
    }
}
