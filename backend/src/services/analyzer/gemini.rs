use super::types::{json_token, ChatMessage, StreamDelta};

/// Gemini SSE chunk → text / thought-part deltas.
pub fn gemini_stream_deltas(json: &serde_json::Value) -> Vec<StreamDelta> {
    let Some(parts) = json
        .pointer("/candidates/0/content/parts")
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for part in parts {
        let Some(text) = json_token(&part["text"]) else {
            continue;
        };
        if part.get("thought").and_then(|v| v.as_bool()) == Some(true) {
            out.push(StreamDelta::Reasoning(text.to_string()));
        } else {
            out.push(StreamDelta::Text(text.to_string()));
        }
    }
    out
}

/// Flatten multi-turn messages into a single Gemini-compatible prompt.
pub(super) fn flatten_messages_for_gemini(system: &str, messages: &[ChatMessage]) -> String {
    let mut parts = Vec::new();
    if !system.trim().is_empty() {
        parts.push(format!("SYSTEM:\n{system}"));
    }
    for message in messages {
        let label = match message.role.as_str() {
            "assistant" => "ASSISTANT",
            "system" => "SYSTEM",
            _ => "USER",
        };
        parts.push(format!("{label}:\n{}", message.content));
    }
    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::super::types::{ChatMessage, StreamDelta};
    use super::{flatten_messages_for_gemini, gemini_stream_deltas};
    use serde_json::json;

    #[test]
    fn flattens_multi_turn_messages_for_gemini() {
        let flat = flatten_messages_for_gemini(
            "rules",
            &[
                ChatMessage::user("first"),
                ChatMessage::assistant("reply"),
                ChatMessage::user("second"),
            ],
        );
        assert!(flat.starts_with("SYSTEM:\nrules"));
        assert!(flat.contains("USER:\nfirst"));
        assert!(flat.contains("ASSISTANT:\nreply"));
        assert!(flat.contains("USER:\nsecond"));
    }

    #[test]
    fn gemini_stream_marks_thought_parts_as_reasoning() {
        let chunk = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        { "text": "thinking aloud", "thought": true },
                        { "text": "hello" }
                    ]
                }
            }]
        });
        assert_eq!(
            gemini_stream_deltas(&chunk),
            vec![
                StreamDelta::Reasoning("thinking aloud".to_string()),
                StreamDelta::Text("hello".to_string()),
            ]
        );
    }
}
