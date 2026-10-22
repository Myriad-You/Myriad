//! Per-handler provider allowance. Charged before HTTP, including retries and
//! format fallbacks; the enclosing Work checkpoint reserves the entire limit.
use super::AiProvider;
use anyhow::{Result, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone)]
pub(crate) struct RequestBudget {
    limit: u64,
    remaining: Arc<AtomicU64>,
}

tokio::task_local! { static REQUEST_BUDGET: RequestBudget; }

impl RequestBudget {
    pub fn new(limit: u64) -> Self {
        Self {
            limit,
            remaining: Arc::new(AtomicU64::new(limit)),
        }
    }
    pub fn charged(&self) -> u64 {
        self.limit
            .saturating_sub(self.remaining.load(Ordering::Relaxed))
    }
    pub async fn scope<F: Future>(&self, future: F) -> F::Output {
        REQUEST_BUDGET.scope(self.clone(), future).await
    }

    fn allocate(&self, input: u64, requested: u64) -> Result<u64> {
        let mut available = self.remaining.load(Ordering::Relaxed);
        loop {
            let output = available.saturating_sub(input).min(requested);
            if output == 0 {
                bail!("The AI tool reached its reserved token budget");
            }
            match self.remaining.compare_exchange_weak(
                available,
                available - input - output,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(output),
                Err(current) => available = current,
            }
        }
    }
}

/// Media uses the same documented token-equivalent estimates as the ledger.
/// Debit before provider I/O so cancellation cannot erase its reservation.
pub(crate) fn charge_units(tokens: u64) -> Result<()> {
    REQUEST_BUDGET
        .try_with(|budget| {
            budget
                .remaining
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                    remaining.checked_sub(tokens)
                })
                .map(|_| ())
                .map_err(|_| anyhow::anyhow!("The AI tool reached its reserved token budget"))
        })
        .unwrap_or(Ok(()))
}

pub(crate) fn prepare(body: &impl Serialize, provider: AiProvider) -> Result<Value> {
    let mut body = serde_json::to_value(body)?;
    REQUEST_BUDGET
        .try_with(|budget| -> Result<()> {
            // One token per UTF-8 byte plus message framing: conservative admission,
            // not a claim about a provider's tokenizer or monetary billing.
            let input = body.to_string().len() as u64 + 256;
            let requested = match provider {
                AiProvider::OpenAI | AiProvider::Anthropic => body["max_tokens"].as_u64(),
                AiProvider::OpenAIResponses => body["max_output_tokens"].as_u64(),
                AiProvider::Gemini => body["generationConfig"]["maxOutputTokens"].as_u64(),
            }
            .unwrap_or(8192);
            let output = budget.allocate(input, requested)?;
            match provider {
                AiProvider::OpenAI | AiProvider::Anthropic => body["max_tokens"] = json!(output),
                AiProvider::OpenAIResponses => body["max_output_tokens"] = json!(output),
                AiProvider::Gemini => {
                    if body["generationConfig"].is_null() {
                        body["generationConfig"] = json!({});
                    }
                    body["generationConfig"]["maxOutputTokens"] = json!(output);
                }
            }
            Ok(())
        })
        .unwrap_or(Ok(()))?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn native_protocols_charge_and_cap_their_output_fields() {
        for (provider, field) in [
            (AiProvider::OpenAIResponses, "max_output_tokens"),
            (AiProvider::Anthropic, "max_tokens"),
        ] {
            let budget = RequestBudget::new(1000);
            budget
                .scope(async {
                    let body =
                        prepare(&json!({"model":"fixture", "input":"hello"}), provider).unwrap();
                    assert!(
                        body[field]
                            .as_u64()
                            .is_some_and(|limit| limit > 0 && limit < 1000)
                    );
                    assert!(prepare(&json!({"input":"again"}), provider).is_err());
                })
                .await;
            assert_eq!(budget.charged(), 1000);
        }
    }

    #[tokio::test]
    async fn media_estimates_are_debited_before_provider_completion() {
        let budget = RequestBudget::new(100);
        budget
            .scope(async {
                charge_units(60).unwrap();
                assert!(charge_units(50).is_err());
                charge_units(40).unwrap();
                assert!(charge_units(1).is_err());
            })
            .await;
        assert_eq!(budget.charged(), 100);
    }

    #[tokio::test]
    async fn analyzer_enforces_allowance_before_http_and_caps_unbounded_chat() {
        use axum::{Json, Router, routing::post};
        use std::sync::atomic::AtomicUsize;
        let hits = Arc::new(AtomicUsize::new(0));
        let observed = hits.clone();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |Json(body): Json<Value>| {
                let hits = observed.clone();
                async move {
                    hits.fetch_add(1, Ordering::Relaxed);
                    assert!(
                        body["max_tokens"]
                            .as_u64()
                            .is_some_and(|limit| limit > 0 && limit < 1000)
                    );
                    Json(json!({"choices":[{"message":{"role":"assistant","content":"done"}}]}))
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let analyzer = super::super::AiAnalyzer::new(
            AiProvider::OpenAI,
            "fixture".into(),
            "fixture".into(),
            Some(format!("http://{address}/v1")),
        )
        .await;
        let budget = RequestBudget::new(1000);
        budget
            .scope(async {
                assert_eq!(analyzer.analyze("Hello").await.unwrap(), "done");
                assert!(analyzer.analyze("Again").await.is_err());
            })
            .await;
        server.abort();
        assert_eq!(hits.load(Ordering::Relaxed), 1);
        assert_eq!(budget.charged(), 1000);
    }

    #[tokio::test]
    async fn nested_requests_share_an_allowance_and_fallback_cannot_drop_output_cap() {
        let budget = RequestBudget::new(1000);
        budget
            .scope(async {
                let first = prepare(
                    &json!({"model":"fixture","messages":[{"role":"user","content":"hello"}]}),
                    AiProvider::OpenAI,
                )
                .unwrap();
                assert!(first["max_tokens"].as_u64().unwrap() < 1000);
                assert!(
                    prepare(
                        &json!({"model":"fixture","messages":[]}),
                        AiProvider::OpenAI
                    )
                    .is_err()
                );
            })
            .await;
        assert_eq!(budget.charged(), 1000);
        let gemini = RequestBudget::new(600);
        gemini
            .scope(async {
                let body = prepare(
                    &json!({"contents":[],"generationConfig":null}),
                    AiProvider::Gemini,
                )
                .unwrap();
                assert!(
                    body["generationConfig"]["maxOutputTokens"]
                        .as_u64()
                        .unwrap()
                        < 600
                );
            })
            .await;
    }
    #[tokio::test]
    async fn oversized_input_is_rejected_before_any_reservation_or_request() {
        let budget = RequestBudget::new(100);
        budget
            .scope(async {
                assert!(prepare(&json!({"messages":[]}), AiProvider::OpenAI).is_err());
            })
            .await;
        assert_eq!(budget.charged(), 0);
    }
}
