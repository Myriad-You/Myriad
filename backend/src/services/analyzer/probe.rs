//! Opt-in model experiments, never a production fallback or a config write.
//! Keep exact request parameters and usage visible without exposing prompts,
//! credentials, provider error bodies, or reasoning text in reports.

use super::{schema::JsonMode, *};
use anyhow::Result;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reasoning {
    Default,
    Disabled,
    Low,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Policy {
    pub reasoning: Reasoning,
    pub temperature: Option<f32>,
    pub max_tokens: u32,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Observation {
    pub headers_ms: Option<u64>,
    pub first_body_ms: Option<u64>,
    pub body_complete_ms: Option<u64>,
    pub status: Option<u16>,
    pub finish: Option<String>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cost: Option<f64>,
}

fn body(
    model: &str,
    system: &str,
    input: &str,
    schema_name: &str,
    schema: &Value,
    policy: Policy,
) -> Value {
    let mut body = json!({
        "model": model,
        "messages": [{"role":"system","content":system},{"role":"user","content":input}],
        "response_format": JsonMode::Structured(Some(schema)).openai_response_format(schema_name),
        "max_tokens": policy.max_tokens,
    });
    match policy.reasoning {
        Reasoning::Default => {}
        Reasoning::Disabled => body["reasoning"] = json!({"enabled":false}),
        Reasoning::Low => body["reasoning"] = json!({"effort":"low"}),
    }
    if let Some(temperature) = policy.temperature {
        body["temperature"] = json!(temperature);
    }
    body
}

impl AiAnalyzer {
    /// Exactly one OpenRouter request. No parameter fallback, model switching,
    /// reasoning-as-answer, global config mutation or cost-ledger writes.
    pub(crate) async fn probe_json(
        &self,
        system: &str,
        input: &str,
        schema_name: &str,
        schema: &Value,
        policy: Policy,
        observation: &mut Observation,
    ) -> Result<String> {
        anyhow::ensure!(
            self.gateway() == Gateway::OpenRouter,
            "probe requires configured OpenRouter gateway"
        );
        let started = std::time::Instant::now();
        let mut response = self
            .client
            .post(openai_chat_completions_url(self.base_url.as_deref()))
            .bearer_auth(self.api_key.as_deref().unwrap_or(""))
            .json(&body(
                &self.model,
                system,
                input,
                schema_name,
                schema,
                policy,
            ))
            .send()
            .await?;
        observation.status = Some(response.status().as_u16());
        observation.headers_ms = Some(started.elapsed().as_millis() as u64);
        anyhow::ensure!(response.status().is_success(), "probe HTTP failure");
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            observation
                .first_body_ms
                .get_or_insert(started.elapsed().as_millis() as u64);
            anyhow::ensure!(
                bytes.len() + chunk.len() <= 65536,
                "probe response exceeded limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        observation.body_complete_ms = Some(started.elapsed().as_millis() as u64);
        let value: Value = serde_json::from_slice(&bytes)?;
        extract_answer(&value, observation)
    }
}

fn extract_answer(value: &Value, observation: &mut Observation) -> Result<String> {
    observation.finish = value
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        .map(|reason| {
            if matches!(reason, "stop" | "length" | "content_filter") {
                reason
            } else {
                "other"
            }
            .into()
        });
    observation.prompt_tokens = value
        .pointer("/usage/prompt_tokens")
        .and_then(Value::as_u64);
    observation.completion_tokens = value
        .pointer("/usage/completion_tokens")
        .and_then(Value::as_u64);
    observation.reasoning_tokens = value
        .pointer("/usage/completion_tokens_details/reasoning_tokens")
        .and_then(Value::as_u64);
    observation.cost = value.pointer("/usage/cost").and_then(Value::as_f64);
    anyhow::ensure!(
        observation.finish.as_deref() == Some("stop"),
        "probe completion did not stop normally"
    );
    value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("probe has no answer content"))
}

#[test]
fn probe_does_not_confuse_truncation_or_reasoning_with_an_answer() {
    let mut observation = Observation::default();
    let mut response = json!({
        "choices":[{"finish_reason":"length","message":{"content":"{\"valence\":1}","reasoning":"private reasoning"}}],
        "usage":{"prompt_tokens":100,"completion_tokens":2048,"completion_tokens_details":{"reasoning_tokens":2040},"cost":0.001}
    });
    assert!(extract_answer(&response, &mut observation).is_err());
    assert_eq!(observation.reasoning_tokens, Some(2040));
    assert_eq!(observation.finish.as_deref(), Some("length"));
    assert!(
        !serde_json::to_string(&observation)
            .unwrap()
            .contains("private")
    );
    response["choices"][0]["finish_reason"] = json!("stop");
    response["choices"][0]["message"]["content"] = Value::Null;
    assert!(extract_answer(&response, &mut observation).is_err());
    response["choices"][0]["message"]["content"] = json!("answer");
    assert_eq!(
        extract_answer(&response, &mut observation).unwrap(),
        "answer"
    );
    assert!(extract_answer(&json!({}), &mut observation).is_err());
    assert_eq!(
        observation.reasoning_tokens, None,
        "missing usage is unknown, not zero"
    );
}

#[test]
fn policies_change_only_explicit_request_parameters() {
    let policy = Policy {
        reasoning: Reasoning::Default,
        temperature: None,
        max_tokens: 2048,
    };
    let original = body("test", "system", "input", "test", &json!({}), policy);
    assert!(original.get("reasoning").is_none());
    assert!(original.get("temperature").is_none());
    for (reasoning, expected) in [
        (Reasoning::Disabled, json!({"enabled":false})),
        (Reasoning::Low, json!({"effort":"low"})),
    ] {
        let mut changed = body(
            "test",
            "system",
            "input",
            "test",
            &json!({}),
            Policy {
                reasoning,
                temperature: Some(0.0),
                ..policy
            },
        );
        assert_eq!(changed["reasoning"], expected);
        assert_eq!(changed["temperature"], 0.0);
        changed.as_object_mut().unwrap().remove("reasoning");
        changed.as_object_mut().unwrap().remove("temperature");
        assert_eq!(changed, original);
    }
}
