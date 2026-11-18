//! Whether the judgment model answers typed decisions or chat, as its source
//! says: OpenRouter lists a decisions model's output as `decisions`. Asked
//! once per model; a judgment never has to guess a model by its name.

use std::time::{Duration, Instant};

use crate::GLOBAL_DYNAMIC_CONFIG;

/// How long asking the source may hold up a judgment.
const ASK_TIMEOUT: Duration = Duration::from_secs(2);
/// A source that could not be reached is asked again after this long.
const RETRY_AFTER: Duration = Duration::from_secs(300);

/// Asks the judgment model's source what kind it is, unless already known.
pub async fn learn_judge_model_kind() {
    let Some(source) = GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .resolve_judge_model_source()
    else {
        return;
    };
    learn(&source.base_url, source.model.trim()).await;
}

/// Asks `base_url`'s source what kind `model` is, once at a time: a
/// judgment that comes while it is being asked waits for the answer (at
/// most [`ASK_TIMEOUT`]) rather than going on not knowing, when it would
/// send a decisions model chat. When the source does not say, the model is
/// marked so (`remember_model_unsaid`): judgments stay on Lite until it does.
async fn learn(base_url: &str, model: &str) {
    // An alias (`~typesafe/jev-latest`) has no listing of its own.
    if model.starts_with('~') || crate::config::model_kind(model).is_some() {
        return;
    }
    static TRIED: tokio::sync::Mutex<Option<(String, Instant)>> =
        tokio::sync::Mutex::const_new(None);
    let mut tried = TRIED.lock().await;
    if crate::config::model_kind(model).is_some()
        || tried
            .as_ref()
            .is_some_and(|(last, at)| last == model && at.elapsed() < RETRY_AFTER)
    {
        return;
    }
    *tried = Some((model.to_owned(), Instant::now()));
    match ask(base_url, model).await {
        Some(decisions) => {
            tracing::info!(model, decisions, "[ModelKind] Judgment model kind learned");
            crate::config::remember_model_kind(model, decisions);
        }
        None => {
            tracing::debug!(model, "[ModelKind] Source did not say; asking again later");
            crate::config::remember_model_unsaid(model);
        }
    }
}

/// `Some(true)` when the source lists the model's output as decisions,
/// `Some(false)` when it lists it otherwise or has no such listing.
async fn ask(base_url: &str, model: &str) -> Option<bool> {
    let origin = url::Url::parse(base_url)
        .ok()?
        .origin()
        .ascii_serialization();
    let response = reqwest::Client::builder()
        .timeout(ASK_TIMEOUT)
        .build()
        .ok()?
        .get(format!("{origin}/api/v1/models/{model}/endpoints"))
        .send()
        .await
        .ok()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Some(false);
    }
    let listing: serde_json::Value = response.error_for_status().ok()?.json().await.ok()?;
    Some(answers_decisions(&listing))
}

fn answers_decisions(listing: &serde_json::Value) -> bool {
    listing["data"]["architecture"]["output_modalities"]
        .as_array()
        .is_some_and(|outputs| outputs.iter().any(|output| output == "decisions"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_model_listed_as_answering_decisions_is_one() {
        let decisions = json!({"data": {"id": "microsoft/microsoft-decision-1", "architecture":
            {"modality": "text->decisions", "input_modalities": ["text"], "output_modalities": ["decisions"]}}});
        let chat = json!({"data": {"id": "openai/gpt-6-luna", "architecture":
            {"modality": "text->text", "output_modalities": ["text"]}}});
        assert!(answers_decisions(&decisions));
        assert!(!answers_decisions(&chat));
        assert!(!answers_decisions(&json!({"error": {"code": 404}})));
    }

    /// A source that answers every ask after a moment: with `listing`, or
    /// an error when there is none.
    async fn source(listing: Option<serde_json::Value>) -> String {
        use axum::response::IntoResponse;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = axum::Router::new().fallback(move || {
            let listing = listing.clone();
            async move {
                tokio::time::sleep(Duration::from_millis(300)).await;
                match listing {
                    Some(listing) => axum::Json(listing).into_response(),
                    None => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                }
            }
        });
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        format!("http://{address}/api/v1")
    }

    /// The model a small judgment would be sent to, `model` judging.
    fn judged_by(model: &str) -> String {
        crate::config::DynamicConfig {
            lite_ai_model: "lite/own-model".to_string(),
            aux_judge_model: model.to_string(),
            ..crate::config::DynamicConfig::default()
        }
        .resolve_lite_judge_ai_config()
        .map(|resolved| resolved.model)
        .unwrap_or_default()
    }

    #[tokio::test]
    async fn a_judgment_that_comes_while_the_source_is_asked_waits_for_its_answer() {
        let model = "acme/slow-decide-1";
        let base = source(Some(
            json!({"data": {"architecture": {"output_modalities": ["decisions"]}}}),
        ))
        .await;
        let (first, second) = tokio::join!(
            async {
                learn(&base, model).await;
                judged_by(model)
            },
            async {
                learn(&base, model).await;
                judged_by(model)
            },
        );
        assert_eq!(first, "lite/own-model");
        assert_eq!(second, "lite/own-model", "the second is not sent it as chat");
    }

    #[tokio::test]
    async fn while_its_source_has_not_said_a_judgment_model_is_not_sent_chat() {
        let model = "acme/unsaid-1";
        let base = source(None).await;
        assert_eq!(judged_by(model), model, "never asked: as before");
        learn(&base, model).await;
        assert_eq!(judged_by(model), "lite/own-model");
        crate::config::remember_model_kind(model, false);
        assert_eq!(judged_by(model), model, "said it is chat: its own");
    }
}
