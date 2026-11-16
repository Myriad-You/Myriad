//! Her acting judged by typed questions about the moment, each answered with
//! how sure the judgment model is: a decisions model (TypeSafe Jev) on its
//! own endpoint, a chat model as structured output. Either way the answers
//! become one performance plan, so everything after it (grounding,
//! capability filter, playback) is shared. The questions and the plan the
//! answers make are the shared core's (`myriad_merope::motion::questions`);
//! asking them is here.
//!
//! Measured on the motion cases (10-12, a bust, two rounds): Jev about 0.6–1.2 s
//! a call; Luna answering about 2–8 s, none timed out; Luna writing the plan
//! instead 4–10 s, two of twenty timed out and one sulk smiled. Wrong answers
//! are mostly unsure, and unsure answers are not acted.

use myriad_merope::RigStateSummary;
use myriad_merope::motion::questions::{asked, plan};
use myriad_merope::motion::{Moment, way};
use serde_json::{Value, json};

mod by_chat;
pub(super) use by_chat::direct_by_chat;

/// The decisions endpoint on the source a chat base URL names
/// (`https://openrouter.ai/api/v1` → `https://openrouter.ai/api/alpha/decisions`).
pub(in crate::services::agent) fn decisions_url(base_url: &str) -> Option<String> {
    let url = url::Url::parse(base_url).ok()?;
    Some(format!(
        "{}/api/alpha/decisions",
        url.origin().ascii_serialization()
    ))
}

/// Asks the decisions model about this moment; the plan as the chat director
/// would have written it, for the shared parser.
pub(super) async fn direct(
    context: &Moment,
    state: Option<&RigStateSummary>,
    persona: Option<&str>,
    source: &crate::config::ResolvedAiConfig,
    timeout: std::time::Duration,
) -> anyhow::Result<String> {
    let started = std::time::Instant::now();
    let (asked_state, asked) = asked(context, state, persona, true);
    let answers = decide(source, asked_state, asked, timeout).await?;
    let mut plan = plan(&answers, context, state, true);
    let left = timeout.saturating_sub(started.elapsed());
    ask_the_way(
        &mut plan,
        &answers,
        context,
        state,
        left,
        |way_state, way, left| decide(source, way_state, way, left),
    )
    .await;
    Ok(plan.to_string())
}

/// A new look: the way there, asked of this change once it is known, in
/// the time `left`; unasked or unanswered, the look comes straight.
async fn ask_the_way<F, Fut>(
    plan: &mut Value,
    answers: &Value,
    context: &Moment,
    state: Option<&RigStateSummary>,
    left: std::time::Duration,
    ask: F,
) where
    F: FnOnce(Value, Value, std::time::Duration) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Value>>,
{
    let Some((from, to)) = way::change(answers, context, state) else {
        return;
    };
    if left <= std::time::Duration::from_millis(400) {
        return;
    }
    let (way_state, way) = way::questions(&from, &to, context, state);
    match tokio::time::timeout(left, ask(way_state, way, left)).await {
        Ok(Ok(way)) => way::lay(plan, &way),
        Ok(Err(error)) => {
            tracing::debug!(%error, "[Merope] the way between looks went unasked")
        }
        Err(_) => tracing::debug!("[Merope] the way between looks took too long"),
    }
}

/// One round of questions to the decisions model; its answers.
async fn decide(
    source: &crate::config::ResolvedAiConfig,
    state: Value,
    questions: Value,
    timeout: std::time::Duration,
) -> anyhow::Result<Value> {
    let url =
        decisions_url(&source.base_url).ok_or_else(|| anyhow::anyhow!("no decisions endpoint"))?;
    let key = source
        .api_key
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no key for the acting source"))?;
    let body = json!({"model": source.model.trim(), "state": state, "questions": questions});
    let response = reqwest::Client::builder()
        .timeout(timeout)
        .build()?
        .post(url)
        .bearer_auth(key)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    let parsed = serde_json::from_str::<Value>(&text);
    let usage = parsed
        .as_ref()
        .map(|value| value["usage"].clone())
        .unwrap_or_default();
    crate::services::ai_cost_ledger::record_ai_tokens_from_attribution(
        &source.provider,
        &source.model,
        usage["input_tokens"].as_i64().unwrap_or(0) as i32,
        usage["output_tokens"].as_i64().unwrap_or(0) as i32,
        if status.is_success() {
            "completed"
        } else {
            "failed"
        },
        (!status.is_success()).then_some(status.as_str()),
    )
    .await;
    anyhow::ensure!(
        status.is_success(),
        "decisions call failed: {status}; {}",
        refusal_shape(&text, key)
    );
    Ok(parsed?["answers"].take())
}

/// What a refused call says about why, fit for the log: the error's type,
/// code, field path and message, cut to a few hundred characters. Fields that
/// echo what was sent (`input`, `value`, the body: her words, the persona)
/// are never kept, and neither is the key or anything shaped like one.
fn refusal_shape(text: &str, key: &str) -> String {
    const KEPT: [&str; 11] = [
        "error", "errors", "detail", "issues", "type", "code", "loc", "path", "param", "msg",
        "message",
    ];
    fn keep(value: &Value, depth: usize) -> Option<Value> {
        match value {
            _ if depth > 6 => None,
            Value::Object(map) => {
                let kept: serde_json::Map<String, Value> = map
                    .iter()
                    .filter(|(name, _)| KEPT.contains(&name.as_str()))
                    .filter_map(|(name, field)| Some((name.clone(), keep(field, depth + 1)?)))
                    .collect();
                (!kept.is_empty()).then_some(Value::Object(kept))
            }
            Value::Array(items) => {
                let kept: Vec<Value> = items
                    .iter()
                    .take(5)
                    .filter_map(|item| keep(item, depth + 1))
                    .collect();
                (!kept.is_empty()).then_some(Value::Array(kept))
            }
            Value::String(text) => Some(Value::String(text.chars().take(160).collect())),
            Value::Number(_) | Value::Bool(_) => Some(value.clone()),
            Value::Null => None,
        }
    }
    static SECRET: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)bearer\s+\S+|\b(?:sk|pk|rk)-[\w-]{6,}|[\w-]{32,}").unwrap()
    });
    let Ok(body) = serde_json::from_str::<Value>(text) else {
        return format!("a body of {} bytes that is not JSON", text.len());
    };
    let Some(shape) = keep(&body, 0) else {
        return "no error fields in the body".into();
    };
    let mut shape = shape.to_string();
    if !key.trim().is_empty() {
        shape = shape.replace(key.trim(), "[key]");
    }
    let shape = SECRET.replace_all(&shape, "[redacted]");
    let mut cut: String = shape.chars().take(400).collect();
    if cut.len() < shape.len() {
        cut.push('…');
    }
    cut
}

/// What the director is asked about a moment, as the semantic eval records it.
#[cfg(test)]
pub(in crate::services::agent) fn semantic_contract(context: &super::MotionContext) -> Value {
    use myriad_merope::motion::questions::{chat_schema, chat_system_prompt};
    let (asked_state, questions) =
        asked(&context.moment(), context.rig_state.as_ref(), None, false);
    // The body and the reply decide which questions are asked; kept beside them.
    let input = json!({"moment": asked_state, "responseText": context.response_text,
        "rig": context.rig_state, "questions": questions});
    json!({"system": chat_system_prompt(&questions), "input": input.to_string(),
        "schema": chat_schema(&questions), "schemaName": "merope_acting_answers"})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::merope::motion::{MotionContext, MotionPhase};

    fn context(reply: &str) -> Moment {
        MotionContext {
            user_id: 1,
            phase: MotionPhase::Delivery,
            mood: serde_json::from_value(
                json!({"before": 60.0, "after": 60.0, "bandBefore": "calm",
                "bandAfter": "calm", "delta": 0.0, "cause": "test", "revision": 1}),
            )
            .unwrap(),
            activity: "talking".into(),
            user_text: "你今天特别可爱。".into(),
            response_text: Some(reply.into()),
            previous_phrases: vec![],
            task_success: None,
            rig_state: None,
            motion_style: "even".into(),
        }
        .moment()
    }

    #[test]
    fn a_decisions_model_is_asked_on_its_source_decisions_endpoint() {
        use crate::config::is_decisions_model;
        assert!(
            is_decisions_model("typesafe/jev-1.13") && is_decisions_model(" ~typesafe/jev-latest")
        );
        assert!(!is_decisions_model("openai/gpt-6-luna"));
        assert_eq!(
            decisions_url("https://openrouter.ai/api/v1").as_deref(),
            Some("https://openrouter.ai/api/alpha/decisions")
        );
    }

    #[tokio::test]
    async fn the_way_is_asked_of_a_new_look_and_never_holds_it() {
        let drowsy = myriad_merope::sanitize_rig_state(&json!({"face": "drowsy",
            "capabilities": ["head-body", "independent-eyes", "mouth-shapes", "sparkles"]}))
        .unwrap();
        let context = context("诶？！真的吗？！");
        let excited = json!({"face": {"choice": "excited", "confidence": 0.9}});
        let surprised = json!({"first": {"choice": "surprised", "confidence": 0.9},
            "then": {"choice": "none", "confidence": 0.9}});
        let quick = std::time::Duration::from_secs(2);

        let mut plan = json!({"baseline": {"face": "excited"}});
        let answer = surprised.clone();
        ask_the_way(
            &mut plan,
            &excited,
            &context,
            Some(&drowsy),
            quick,
            |_, way, _| async move {
                assert!(way["first"]["criteria"]["surprised"].is_string());
                Ok(answer)
            },
        )
        .await;
        assert_eq!(plan["baseline"]["via"], json!(["surprised"]));

        // A slow answer is not waited for: the look comes straight.
        let mut plan = json!({"baseline": {"face": "excited"}});
        let answer = surprised.clone();
        ask_the_way(
            &mut plan,
            &excited,
            &context,
            Some(&drowsy),
            std::time::Duration::from_millis(500),
            |_, _, _| async move {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                Ok(answer)
            },
        )
        .await;
        assert!(plan["baseline"]["via"].is_null());

        // The same look again has no way to ask.
        let mut plan = json!({"baseline": {"face": "drowsy"}});
        let same = json!({"face": {"choice": "drowsy", "confidence": 0.9}});
        ask_the_way(
            &mut plan,
            &same,
            &context,
            Some(&drowsy),
            quick,
            |_, _, _| async { panic!("asked of no change") },
        )
        .await;
        assert!(plan["baseline"]["via"].is_null());
    }

    #[test]
    fn a_refusal_keeps_its_shape_and_never_what_was_sent() {
        let said = "今天你特别可爱，我想一直看着你";
        let key = "or-v1-0123456789abcdef0123456789abcdef";
        // A validation refusal that echoes the request back.
        let refused = json!({"detail": [{
            "type": "string_too_long",
            "loc": ["body", "state", "responseText"],
            "msg": "String should have at most 500 characters",
            "input": said,
            "ctx": {"max_length": 500, "given": said},
            "url": "https://errors.example/string_too_long",
        }], "body": {"state": {"responseText": said}}})
        .to_string();
        let shape = refusal_shape(&refused, key);
        assert!(shape.contains("string_too_long"), "{shape}");
        assert!(shape.contains("responseText"), "{shape}");
        assert!(shape.contains("at most 500"), "{shape}");
        assert!(!shape.contains(said), "{shape}");
        assert!(
            !shape.contains("input") && !shape.contains("ctx"),
            "{shape}"
        );

        // An error object that names the key or a bearer header.
        let named = json!({"error": {
            "type": "invalid_request_error",
            "code": 422,
            "param": "questions[3].options",
            "message": format!("bad request with Authorization: Bearer {key} and sk-live-abcdef123456"),
            "value": said,
        }})
        .to_string();
        let shape = refusal_shape(&named, key);
        assert!(shape.contains("questions[3].options"), "{shape}");
        assert!(shape.contains("invalid_request_error"), "{shape}");
        assert!(
            !shape.contains(key) && !shape.contains("0123456789abcdef"),
            "{shape}"
        );
        assert!(!shape.contains("sk-live"), "{shape}");
        assert!(!shape.contains(said), "{shape}");

        // Long messages are cut; a body that is not JSON is only measured.
        let long = json!({"error": {"message": "x ".repeat(2000)}}).to_string();
        assert!(refusal_shape(&long, key).chars().count() <= 401);
        let html = format!("<html>{said}</html>");
        let shape = refusal_shape(&html, key);
        assert!(
            !shape.contains(said) && shape.contains("not JSON"),
            "{shape}"
        );
        assert_eq!(refusal_shape("{}", key), "no error fields in the body");
    }
}
