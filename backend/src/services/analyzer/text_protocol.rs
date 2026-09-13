use anyhow::Result;
use serde_json::{json, Value};

use super::{AiProvider, ChatMessage, StreamDelta};

pub(crate) fn endpoint(provider: AiProvider, base_url: Option<&str>) -> String {
    let (default_base, suffix) = match provider {
        AiProvider::OpenAIResponses => ("https://api.openai.com/v1", "/responses"),
        AiProvider::Anthropic => ("https://api.anthropic.com/v1", "/messages"),
        _ => unreachable!("text protocol endpoint called for unsupported provider"),
    };
    let base = base_url.unwrap_or(default_base).trim().trim_end_matches('/');
    if base.ends_with(suffix) {
        base.to_string()
    } else if matches!(
        (provider, base),
        (
            AiProvider::OpenAIResponses,
            "https://api.openai.com" | "http://api.openai.com"
        )
            | (AiProvider::Anthropic, "https://api.anthropic.com" | "http://api.anthropic.com")
    ) {
        format!("{base}/v1{suffix}")
    } else {
        format!("{base}{suffix}")
    }
}

pub(super) fn request_body(
    provider: AiProvider,
    model: &str,
    system: &str,
    messages: &[ChatMessage],
    response_format: Option<Value>,
    max_tokens: Option<u32>,
    stream: bool,
) -> Value {
    match provider {
        AiProvider::OpenAIResponses => {
            let mut body = json!({
                "model": model,
                "input": messages.iter().map(|message| json!({
                    "role": responses_role(&message.role),
                    "content": message.content,
                })).collect::<Vec<_>>(),
                "store": false,
            });
            let object = body.as_object_mut().expect("responses body");
            if !system.trim().is_empty() {
                object.insert("instructions".into(), json!(system));
            }
            if let Some(format) = response_format.and_then(responses_text_format) {
                object.insert("text".into(), json!({ "format": format }));
            }
            if let Some(limit) = max_tokens {
                object.insert("max_output_tokens".into(), json!(limit));
            }
            if stream {
                object.insert("stream".into(), json!(true));
            }
            body
        }
        AiProvider::Anthropic => {
            let system = messages
                .iter()
                .filter(|message| message.role == "system")
                .map(|message| message.content.as_str())
                .chain((!system.trim().is_empty()).then_some(system))
                .collect::<Vec<_>>()
                .join("\n\n");
            let mut body = json!({
                "model": model,
                "max_tokens": max_tokens.unwrap_or(4096),
                "messages": messages.iter().filter(|message| message.role != "system").map(|message| json!({
                    "role": anthropic_role(&message.role),
                    "content": message.content,
                })).collect::<Vec<_>>(),
            });
            let object = body.as_object_mut().expect("anthropic body");
            if !system.trim().is_empty() {
                object.insert("system".into(), json!(system));
            }
            if stream {
                object.insert("stream".into(), json!(true));
            }
            body
        }
        _ => unreachable!("request body called for unsupported provider"),
    }
}

fn anthropic_role(role: &str) -> &str {
    if role == "assistant" {
        "assistant"
    } else {
        "user"
    }
}

fn responses_role(role: &str) -> &str {
    match role {
        "assistant" => "assistant",
        "system" => "system",
        "developer" => "developer",
        _ => "user",
    }
}

fn responses_text_format(openai: Value) -> Option<Value> {
    let mut object = openai.as_object()?.clone();
    if object.get("type").and_then(Value::as_str) == Some("json_schema") {
        let schema = object.remove("json_schema")?;
        let schema = schema.as_object()?;
        return Some(json!({
            "type": "json_schema",
            "name": schema.get("name")?,
            "schema": schema.get("schema")?,
            "strict": schema.get("strict").cloned().unwrap_or(json!(false)),
        }));
    }
    Some(Value::Object(object))
}

pub(crate) fn response_text(provider: AiProvider, body: &Value) -> Result<String> {
    let text = match provider {
        AiProvider::OpenAIResponses => body
            .get("output_text")
            .and_then(Value::as_str)
            .or_else(|| {
                body.get("output")?.as_array()?.iter().find_map(|item| {
                    item.get("content")?.as_array()?.iter().find_map(|part| {
                        part.get("text").and_then(Value::as_str)
                    })
                })
            }),
        AiProvider::Anthropic => body.get("content").and_then(Value::as_array).and_then(|items| {
            items.iter().find_map(|item| item.get("text").and_then(Value::as_str))
        }),
        _ => None,
    };
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("AI provider returned no text content"))
}

pub(super) fn stream_deltas(provider: AiProvider, body: &Value) -> Vec<StreamDelta> {
    match provider {
        AiProvider::OpenAIResponses => super::openai::openai_stream_deltas(body),
        AiProvider::Anthropic => {
            let delta = &body["delta"];
            match delta.get("type").and_then(Value::as_str) {
                Some("text_delta") => delta
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| vec![StreamDelta::Text(text.to_string())])
                    .unwrap_or_default(),
                Some("thinking_delta") => delta
                    .get("thinking")
                    .and_then(Value::as_str)
                    .map(|text| vec![StreamDelta::Reasoning(text.to_string())])
                    .unwrap_or_default(),
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocols_have_distinct_endpoints_and_shapes() {
        assert_eq!(
            endpoint(AiProvider::OpenAIResponses, Some("https://llm.example/v1")),
            "https://llm.example/v1/responses"
        );
        assert_eq!(
            endpoint(AiProvider::Anthropic, Some("https://claude.example/v1/messages")),
            "https://claude.example/v1/messages"
        );
        let messages = vec![ChatMessage::user("hello")];
        let responses = request_body(
            AiProvider::OpenAIResponses,
            "gpt-x",
            "be concise",
            &messages,
            Some(json!({"type":"json_object"})),
            Some(50),
            false,
        );
        assert_eq!(responses["instructions"], "be concise");
        assert_eq!(responses["store"], false);
        assert_eq!(responses["max_output_tokens"], 50);
        assert_eq!(responses["text"]["format"]["type"], "json_object");

        let anthropic = request_body(
            AiProvider::Anthropic,
            "claude-x",
            "be concise",
            &messages,
            None,
            None,
            false,
        );
        assert_eq!(anthropic["system"], "be concise");
        assert_eq!(anthropic["max_tokens"], 4096);
        assert!(anthropic.get("response_format").is_none());
    }

    #[test]
    fn protocols_extract_blocking_and_streaming_text() {
        assert_eq!(
            response_text(
                AiProvider::OpenAIResponses,
                &json!({"output":[{"content":[{"type":"output_text","text":"done"}]}]})
            )
            .unwrap(),
            "done"
        );
        assert_eq!(
            response_text(
                AiProvider::Anthropic,
                &json!({"content":[{"type":"text","text":"hello"}]})
            )
            .unwrap(),
            "hello"
        );
        assert_eq!(
            stream_deltas(
                AiProvider::Anthropic,
                &json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"a"}})
            ),
            vec![StreamDelta::Text("a".into())]
        );
    }
}
