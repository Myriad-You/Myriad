use serde_json::{json, Map, Value};

use crate::sanitize_onboarding_tags;

const MAX_PERSONA_LIST_ITEMS: usize = 12;

pub fn fallback_persona_draft(name: &str, language: &str, tags: &[String]) -> Value {
    let tags = sanitize_onboarding_tags(tags);
    let summary = fallback_summary(name, language, &tags);
    json!({
        "displayName": bounded_text(name, 50),
        "summary": summary,
        "temperament": tags,
        "likes": [],
        "drives": [],
        "socialStyle": "",
        "speechStyle": "",
        "language": bounded_text(language, 16),
        "draftSource": "fallback",
    })
}

pub fn sanitize_persona_draft(value: &Value, fallback: &Value) -> Option<Value> {
    let source = value.get("persona").unwrap_or(value).as_object()?;
    let mut result = fallback.as_object()?.clone();

    replace_text(&mut result, source, "summary", &["summary"], 1_200);
    replace_list(
        &mut result,
        source,
        "temperament",
        &["temperament", "traits"],
    );
    replace_list(&mut result, source, "likes", &["likes"]);
    replace_list(&mut result, source, "drives", &["drives", "motivations"]);
    replace_text(
        &mut result,
        source,
        "socialStyle",
        &["socialStyle", "social_style"],
        500,
    );
    replace_text(
        &mut result,
        source,
        "speechStyle",
        &["speechStyle", "speech_style", "voice"],
        500,
    );

    result.remove("visualIdentity");
    result.remove("visual_identity");
    result.insert("draftSource".into(), json!("ai"));
    Some(Value::Object(result))
}

pub fn persona_draft_is_complete(value: &Value) -> bool {
    let Some(source) = value.get("persona").unwrap_or(value).as_object() else {
        return false;
    };
    let summary_ready = source
        .get("summary")
        .and_then(Value::as_str)
        .is_some_and(|summary| summary.trim().chars().count() >= 8);
    let temperament_ready =
        list_from(source, &["temperament", "traits"]).is_some_and(|items| !items.is_empty());
    let likes_ready = list_from(source, &["likes"]).is_some_and(|items| !items.is_empty());
    let drives_ready =
        list_from(source, &["drives", "motivations"]).is_some_and(|items| !items.is_empty());
    let social_ready = source
        .get("socialStyle")
        .or_else(|| source.get("social_style"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let speech_ready = source
        .get("speechStyle")
        .or_else(|| source.get("speech_style"))
        .or_else(|| source.get("voice"))
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    summary_ready
        && temperament_ready
        && likes_ready
        && drives_ready
        && social_ready
        && speech_ready
}

fn fallback_summary(name: &str, language: &str, tags: &[String]) -> String {
    let name = bounded_text(name, 50);
    let traits = tags.iter().take(5).cloned().collect::<Vec<_>>();
    if traits.is_empty() {
        return match language {
            "ja-JP" => format!("{name}は、これから個性を育てていきます。"),
            "en-US" => format!("{name}'s personality will grow through shared experiences."),
            "zh-TW" => format!("{name}會在相處中逐漸形成獨特個性。"),
            _ => format!("{name}会在相处中逐渐形成独特个性。"),
        };
    }
    match language {
        "ja-JP" => format!("{name}は、{}という気質を持っています。", traits.join("、")),
        "en-US" => format!("{name} has a {} temperament.", traits.join(", ")),
        "zh-TW" => format!("{name}帶有{}氣質。", traits.join("、")),
        _ => format!("{name}带有{}气质。", traits.join("、")),
    }
}

fn replace_text(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    target_key: &str,
    source_keys: &[&str],
    max_chars: usize,
) {
    if let Some(value) = source_keys
        .iter()
        .find_map(|key| source.get(*key).and_then(Value::as_str))
        .map(|value| bounded_text(value, max_chars))
        .filter(|value| !value.is_empty())
    {
        target.insert(target_key.to_string(), json!(value));
    }
}

fn replace_list(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    target_key: &str,
    source_keys: &[&str],
) {
    if let Some(values) = list_from(source, source_keys).filter(|values| !values.is_empty()) {
        target.insert(target_key.to_string(), json!(values));
    }
}

fn list_from(source: &Map<String, Value>, keys: &[&str]) -> Option<Vec<String>> {
    let value = keys.iter().find_map(|key| source.get(*key))?;
    let raw = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>(),
        Value::String(value) => value
            .split(['、', ',', '，', ';', '/', '|'])
            .map(str::to_string)
            .collect(),
        _ => return None,
    };
    let mut sanitized = Vec::new();
    for item in raw {
        let item = bounded_text(&item, 120);
        if item.is_empty() || sanitized.iter().any(|existing| existing == &item) {
            continue;
        }
        sanitized.push(item);
        if sanitized.len() == MAX_PERSONA_LIST_ITEMS {
            break;
        }
    }
    Some(sanitized)
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_persona_is_character_only() {
        let draft = fallback_persona_draft("Nova", "en-US", &["calm".into(), "curious".into()]);
        assert!(!persona_draft_is_complete(&draft));
        assert_eq!(draft["draftSource"], "fallback");
        assert!(draft.get("visualIdentity").is_none());
        for forbidden in ["room", "world", "furniture", "environment"] {
            assert!(draft.get(forbidden).is_none());
        }
    }

    #[test]
    fn sanitizes_model_persona_and_drops_visual_fields() {
        let fallback = fallback_persona_draft("Nova", "zh-CN", &["安静".into()]);
        let draft = sanitize_persona_draft(
            &json!({
                "persona": {
                    "summary": "安静但会认真回应重要事情。",
                    "temperament": ["克制", "细心", "克制"],
                    "likes": "雨声、旧书",
                    "drives": ["理解别人"],
                    "socialStyle": "先听，再回应。",
                    "speechStyle": "简洁但温和。",
                    "visualIdentity": { "hair": "银灰短发" }
                }
            }),
            &fallback,
        )
        .unwrap();
        assert_eq!(draft["displayName"], "Nova");
        assert_eq!(draft["temperament"], json!(["克制", "细心"]));
        assert!(draft.get("visualIdentity").is_none());
        assert!(persona_draft_is_complete(&draft));
        assert_eq!(draft["draftSource"], "ai");
    }

    #[test]
    fn incomplete_model_payload_is_detected_before_sanitized_fallback() {
        assert!(!persona_draft_is_complete(&json!({"summary": "短"})));
        assert!(!persona_draft_is_complete(
            &json!({"temperament": ["calm"]})
        ));
    }
}
