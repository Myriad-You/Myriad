//! Shared perception rendering. Chat and consciousness must honor the same
//! privacy rule: `local` never exposes `summary`.

use serde_json::{Map, Value};

/// Cap for perception items. Frontend `MAX_PERCEPTION_ITEMS` must match.
pub const MAX_PERCEPTION_ITEMS: usize = 12;

pub fn perception_facts_line(obj: &Map<String, Value>) -> String {
    let Some(Value::Object(facts)) = obj.get("safeFacts") else {
        return String::new();
    };
    let mut parts = Vec::new();
    for (key, value) in facts.iter().take(12) {
        let shown = match value {
            Value::String(text) => text.chars().take(120).collect::<String>(),
            Value::Bool(flag) => flag.to_string(),
            Value::Number(number) => number.to_string(),
            _ => continue,
        };
        let name: String = key.chars().take(40).collect();
        parts.push(format!("{name}={shown}"));
    }
    parts.join(" ")
}

/// Text a reader may use. `privacy: "local"` yields only `safeFacts`.
pub fn perception_reader_text(obj: &Map<String, Value>) -> String {
    let privacy = obj.get("privacy").and_then(Value::as_str).unwrap_or("");
    if privacy == "local" {
        perception_facts_line(obj)
    } else {
        obj.get("summary")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .take(400)
            .collect()
    }
}
