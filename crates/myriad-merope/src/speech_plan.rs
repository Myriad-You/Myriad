use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::PERFORMANCE_PHRASE_INTENTS;

/// An exact response fragment and its delivery intent, never rig coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeechPhrase {
    pub text: String,
    pub intent: String,
}

pub fn grounded_speech_phrases(value: &Value, response: Option<&str>) -> Vec<SpeechPhrase> {
    let (Some(items), Some(response)) = (value.as_array(), response) else {
        return Vec::new();
    };
    let mut phrases: Vec<SpeechPhrase> = Vec::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for item in items.iter().take(6) {
        let Ok(phrase) = serde_json::from_value::<SpeechPhrase>(item.clone()) else {
            continue;
        };
        if !(2..=120).contains(&phrase.text.chars().count())
            || phrase.text.trim() != phrase.text
            || !PERFORMANCE_PHRASE_INTENTS.contains(&phrase.intent.as_str())
        {
            continue;
        }
        let Some(start) = response.find(&phrase.text) else {
            continue;
        };
        let end = start + phrase.text.len();
        // Search one character later, not after the match: overlapping
        // occurrences (e.g. 哈哈 in 哈哈哈) are ambiguous too.
        let next_char = start + phrase.text.chars().next().unwrap().len_utf8();
        if response[next_char..].contains(&phrase.text)
            || ranges
                .iter()
                .any(|(left, right)| start < *right && *left < end)
        {
            continue;
        }
        ranges.push((start, end));
        phrases.push(phrase);
    }
    phrases
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn phrase_plan_requires_unique_response_evidence_and_bounded_known_intents() {
        let phrases = grounded_speech_phrases(
            &json!([
                {"text":"也许可以。", "intent":"hesitate"},
                {"text":"可以", "intent":"tease"},
                {"text":"你觉得呢？", "intent":"check-in"},
                {"text":"不存在", "intent":"laugh"},
                {"text":"然后呢？", "intent":"driver"}
            ]),
            Some("也许可以。你觉得呢？然后呢？"),
        );
        assert_eq!(phrases.len(), 2);
        assert_eq!(phrases[0].intent, "hesitate");
        assert_eq!(phrases[1].intent, "check-in");
        assert!(grounded_speech_phrases(
            &json!([{"text":"你好", "intent":"ask"}]),
            Some("你好，你好")
        )
        .is_empty());
        assert!(grounded_speech_phrases(&json!([]), None).is_empty());
        assert!(grounded_speech_phrases(
            &json!([{"text":"哈哈", "intent":"laugh"}]),
            Some("哈哈哈")
        )
        .is_empty());
    }
}
