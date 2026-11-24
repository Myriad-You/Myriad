//! What is sent for a JSON answer, each way it can be held to its schema:
//! the whole body, to the letter.

use super::{AiAnalyzer, AiProvider, JsonShape};
use serde_json::{Value, json};
use std::time::Duration;

/// Every body `ask` sent to an analyzer of `provider`'s.
async fn sent(
    provider: AiProvider,
    ask: impl AsyncFnOnce(&AiAnalyzer) -> anyhow::Result<String>,
) -> Vec<Value> {
    use axum::{Json, Router, http::StatusCode, routing::post};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (tell, mut told) = tokio::sync::mpsc::unbounded_channel::<Value>();
    let server = tokio::spawn(async move {
        let answer = post(move |Json(body): Json<Value>| {
            let _ = tell.send(body);
            // Refused for who asks: nothing is asked again.
            async { StatusCode::UNAUTHORIZED }
        });
        axum::serve(listener, Router::new().fallback(answer))
            .await
            .unwrap();
    });
    let analyzer = AiAnalyzer::new_with_timeout(
        provider,
        "k".into(),
        "m".into(),
        Some(format!("http://{address}/v1")),
        Duration::from_secs(2),
    )
    .await;
    let _ = ask(&analyzer).await;
    server.abort();
    let mut bodies = Vec::new();
    while let Ok(body) = told.try_recv() {
        bodies.push(body);
    }
    bodies
}

fn schema() -> Value {
    json!({
        "type": "object", "additionalProperties": false, "required": ["read"],
        "properties": {"read": {"type": "boolean"}},
    })
}

const IN_PROMPT: &str = "hello\n\nReturn one valid JSON value only, without Markdown fences. \
     The JSON value must satisfy this schema:\n\
     {\"additionalProperties\":false,\"properties\":{\"read\":{\"type\":\"boolean\"}},\
     \"required\":[\"read\"],\"type\":\"object\"}";

/// What a step that says nothing of it sends.
#[tokio::test]
async fn a_step_that_says_nothing_of_it_is_held_by_the_provider_as_ever() {
    let schema = schema();
    for (provider, body) in [
        (
            AiProvider::OpenAI,
            json!({
                "model": "m",
                "messages": [
                    {"role": "system", "content": "s"},
                    {"role": "user", "content": "hello"},
                ],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {"name": "n", "schema": schema, "strict": false},
                },
            }),
        ),
        (
            AiProvider::OpenAIResponses,
            json!({
                "model": "m",
                "input": [{"role": "user", "content": "hello"}],
                "store": false,
                "instructions": "s",
                "text": {"format": {
                    "type": "json_schema", "name": "n", "schema": schema, "strict": false,
                }},
            }),
        ),
        (
            AiProvider::Anthropic,
            json!({
                "model": "m",
                "max_tokens": 4096,
                "messages": [{"role": "user", "content": IN_PROMPT}],
                "system": "s",
            }),
        ),
    ] {
        let bodies = sent(provider, async |analyzer| {
            (analyzer.analyze_json("s", "hello", "n", Some(&schema))).await
        })
        .await;
        assert_eq!(bodies, [body], "{provider:?}");
    }
}

/// Said in the prompt alone, whatever answers.
#[tokio::test]
async fn a_step_that_says_so_has_its_schema_in_the_prompt_alone() {
    let schema = schema();
    for (provider, body) in [
        (
            AiProvider::OpenAI,
            json!({
                "model": "m",
                "messages": [
                    {"role": "system", "content": "s"},
                    {"role": "user", "content": IN_PROMPT},
                ],
            }),
        ),
        (
            AiProvider::OpenAIResponses,
            json!({
                "model": "m",
                "input": [{"role": "user", "content": IN_PROMPT}],
                "store": false,
                "instructions": "s",
            }),
        ),
        (
            AiProvider::Anthropic,
            json!({
                "model": "m",
                "max_tokens": 4096,
                "messages": [{"role": "user", "content": IN_PROMPT}],
                "system": "s",
            }),
        ),
    ] {
        let bodies = sent(provider, async |analyzer| {
            (analyzer.analyze_json_as("s", "hello", "n", Some(&schema), JsonShape::InPrompt)).await
        })
        .await;
        assert_eq!(bodies, [body], "{provider:?}");
    }
}
