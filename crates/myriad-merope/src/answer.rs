//! Reading a model's answer: the JSON object in it, or the whole of it.

use serde::Deserialize;

/// The answer as `T`: the JSON object in it (fenced or not), else the whole.
pub fn parse<T: for<'de> Deserialize<'de>>(raw: &str) -> Option<T> {
    let json = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim());
    serde_json::from_str(json.as_deref().unwrap_or(raw.trim())).ok()
}

#[cfg(test)]
mod tests {
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Pick {
        choice: Option<usize>,
    }

    #[test]
    fn an_answer_is_read_from_the_json_in_it() {
        assert_eq!(
            super::parse::<Pick>("好的：\n```json\n{\"choice\":2}\n```"),
            Some(Pick { choice: Some(2) })
        );
        assert_eq!(
            super::parse::<Pick>("{\"choice\":null}"),
            Some(Pick { choice: None })
        );
        assert_eq!(super::parse::<Pick>("不知道"), None);
    }
}
