//! Embeddings on an OpenAI-compatible endpoint (`/embeddings`): what a text
//! means, as a vector, for recall to find it by meaning as well as by words.

use anyhow::{Context, Result};
use serde::Deserialize;

use super::client::AiAnalyzer;
use super::types::AiProvider;

/// `…/chat/completions` of the same endpoint, as `…/embeddings`.
fn embeddings_url(base_url: Option<&str>) -> String {
    let chat = super::openai::openai_chat_completions_url(base_url);
    match chat.strip_suffix("/chat/completions") {
        Some(root) => format!("{root}/embeddings"),
        None => format!("{chat}/embeddings"),
    }
}

#[derive(Deserialize)]
struct Embedded {
    index: usize,
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct EmbeddingsResponse {
    data: Vec<Embedded>,
}

/// Most a response may carry: a batch of long vectors in JSON text.
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

impl AiAnalyzer {
    /// One vector for each of `inputs`, in their order. Only OpenAI-compatible
    /// endpoints embed here.
    pub async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        if self.provider != AiProvider::OpenAI {
            anyhow::bail!("embeddings need an OpenAI-compatible endpoint");
        }
        let url = embeddings_url(self.base_url.as_deref());
        let body = serde_json::json!({
            "model": self.model,
            "input": inputs,
            "encoding_format": "float",
        });
        let input_chars = inputs.iter().map(String::len).sum::<usize>();
        let result = async {
            let response = self
                .authenticate(self.client.post(&url))
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
                .with_context(|| {
                    format!(
                        "Failed to send embeddings request (endpoint: {url}, model: {})",
                        self.model
                    )
                })?;
            let status = response.status();
            let bytes =
                crate::services::outbound_security::read_limited_body(response, MAX_RESPONSE_BYTES)
                    .await
                    .map_err(|error| anyhow::anyhow!(error))?;
            if !status.is_success() {
                anyhow::bail!(super::openai::format_openai_compatible_http_error(
                    status,
                    &url,
                    &self.model,
                    &String::from_utf8_lossy(&bytes[..bytes.len().min(64 * 1024)]),
                ));
            }
            let parsed: EmbeddingsResponse =
                serde_json::from_slice(&bytes).context("Failed to parse embeddings JSON")?;
            ordered(parsed.data, inputs.len())
        }
        .await;
        crate::services::ai_cost_ledger::record_ai_call_from_attribution(
            self.provider.as_str(),
            &self.model,
            input_chars,
            0,
            if result.is_ok() {
                "completed"
            } else {
                "failed"
            },
            result.is_err().then_some("AI_PROVIDER_ERROR"),
        )
        .await;
        result
    }
}

/// The vectors in the order asked, each there once and of one length.
fn ordered(data: Vec<Embedded>, expected: usize) -> Result<Vec<Vec<f32>>> {
    let mut out: Vec<Option<Vec<f32>>> = vec![None; expected];
    for item in data {
        let slot = out
            .get_mut(item.index)
            .context("embeddings response names an input that was not sent")?;
        *slot = Some(item.embedding);
    }
    let out: Vec<Vec<f32>> = out
        .into_iter()
        .collect::<Option<_>>()
        .context("embeddings response is missing an input")?;
    let length = out.first().map_or(0, Vec::len);
    if length == 0 || out.iter().any(|vector| vector.len() != length) {
        anyhow::bail!("embeddings response has empty or uneven vectors");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embeddings_endpoint_sits_beside_chat() {
        assert_eq!(
            embeddings_url(Some("https://openrouter.ai/api/v1")),
            "https://openrouter.ai/api/v1/embeddings"
        );
        assert_eq!(
            embeddings_url(Some("https://openrouter.ai")),
            "https://openrouter.ai/api/v1/embeddings"
        );
        assert_eq!(
            embeddings_url(Some("https://gateway.example/v1/chat/completions")),
            "https://gateway.example/v1/embeddings"
        );
    }

    #[test]
    fn vectors_come_back_in_the_order_asked() {
        let data = vec![
            Embedded {
                index: 1,
                embedding: vec![0.0, 1.0],
            },
            Embedded {
                index: 0,
                embedding: vec![1.0, 0.0],
            },
        ];
        assert_eq!(
            ordered(data, 2).unwrap(),
            vec![vec![1.0, 0.0], vec![0.0, 1.0]]
        );
        assert!(
            ordered(
                vec![Embedded {
                    index: 0,
                    embedding: vec![1.0]
                }],
                2
            )
            .is_err()
        );
        assert!(
            ordered(
                vec![Embedded {
                    index: 3,
                    embedding: vec![1.0]
                }],
                1
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn embed_posts_the_batch_and_reads_it_back() {
        use axum::{Json, Router, routing::post};
        let app = Router::new().route(
            "/v1/embeddings",
            post(|Json(body): Json<serde_json::Value>| async move {
                assert_eq!(body["model"], "fixture-embed");
                let inputs = body["input"].as_array().unwrap().len();
                Json(serde_json::json!({
                    "data": (0..inputs).rev().map(|index| serde_json::json!({
                        "index": index, "embedding": [index as f32, 1.0]
                    })).collect::<Vec<_>>()
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let analyzer = AiAnalyzer::new(
            AiProvider::OpenAI,
            "fixture".into(),
            "fixture-embed".into(),
            Some(format!("http://{address}/v1")),
        )
        .await;
        let vectors = analyzer
            .embed(&["猫".to_string(), "cat".to_string()])
            .await
            .unwrap();
        server.abort();
        assert_eq!(vectors, vec![vec![0.0, 1.0], vec![1.0, 1.0]]);
    }
}
