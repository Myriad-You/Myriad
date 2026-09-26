//! Asking a model on her behalf: which voice, billed to whom under what
//! name, and the answer read as the JSON it was asked for.
//!
//! A judgment (what to pick, whether a step is worth taking, whether a guess
//! held) goes to the fast judging model. Anything in her own words goes to
//! her voice, thinking a little, as her chat replies do.

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Voice {
    Judge,
    Hers,
}

/// The raw answer, or none if no model is set up, it failed or timed out.
#[allow(clippy::too_many_arguments)]
pub(super) async fn ask_raw(
    voice: Voice,
    timeout: Duration,
    owner: i32,
    operation: &'static str,
    system: &str,
    input: &str,
    schema_name: &str,
    schema: &Value,
) -> Option<String> {
    let analyzer = match voice {
        Voice::Judge => {
            crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(timeout)).await?
        }
        Voice::Hers => {
            crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(timeout))
                .await?
                .with_light_thinking()
        }
    };
    crate::services::ai_cost_ledger::with_site_ai_ledger(
        owner,
        "merope",
        operation,
        analyzer.analyze_json(system, input, schema_name, Some(schema)),
    )
    .await
    .ok()
}

/// [`ask_raw`], read as `T`.
#[allow(clippy::too_many_arguments)]
pub(super) async fn ask<T: for<'de> Deserialize<'de>>(
    voice: Voice,
    timeout: Duration,
    owner: i32,
    operation: &'static str,
    system: &str,
    input: &str,
    schema_name: &str,
    schema: &Value,
) -> Option<T> {
    parse(
        &ask_raw(
            voice,
            timeout,
            owner,
            operation,
            system,
            input,
            schema_name,
            schema,
        )
        .await?,
    )
}

/// A model's answer as `T`: the JSON object in it, or the whole of it.
pub(super) fn parse<T: for<'de> Deserialize<'de>>(raw: &str) -> Option<T> {
    let json = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim());
    serde_json::from_str(json.as_deref().unwrap_or(raw.trim())).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_answer_is_read_from_the_json_in_it() {
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct Pick {
            choice: Option<usize>,
        }
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
