//! Asking a model on her behalf: which voice, billed to whom under what
//! name, and the answer as JSON, as text, or as something she says.
//!
//! A judgment (what to pick, whether a step is worth taking, whether a guess
//! held) goes to the fast judging model. Anything in her own words goes to
//! her voice, thinking a little as her chat replies do, or, where a call
//! needs it, as long as the provider likes.
//!
//! This is the one place Merope reaches the model and the cost ledger. The
//! live appraisal keeps its own analyzer for its timing probes.

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::services::analyzer::AiAnalyzer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Voice {
    /// The fast judging model.
    Judge,
    /// Her voice, thinking a little.
    Hers,
    /// Her voice, thinking as long as the provider likes.
    HersAtLength,
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(45);

/// Safe, structured outcome. Provider bodies and credentials never enter job records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Failure {
    Unavailable,
    Timeout,
    Transport,
    Upstream,
    RateLimited,
    Rejected,
    InvalidOutput,
    StreamInterrupted,
    Quota,
    Unknown,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "model_unavailable",
            Self::Timeout => "model_timeout",
            Self::Transport => "model_transport",
            Self::Upstream => "model_upstream",
            Self::RateLimited => "model_rate_limited",
            Self::Rejected => "model_rejected",
            Self::InvalidOutput => "model_invalid_output",
            Self::StreamInterrupted => "model_stream_interrupted",
            Self::Quota => "model_quota",
            Self::Unknown => "model_unknown_failure",
        })
    }
}
impl std::error::Error for Failure {}

impl Failure {
    pub(super) fn retryable(self) -> bool {
        matches!(
            self,
            Self::Timeout | Self::Transport | Self::Upstream | Self::StreamInterrupted
        )
    }

    fn status(status: reqwest::StatusCode) -> Self {
        match status.as_u16() {
            408 | 504 => Self::Timeout,
            429 => Self::RateLimited,
            500..=599 => Self::Upstream,
            _ => Self::Rejected,
        }
    }

    pub(super) fn classify(error: &anyhow::Error) -> Self {
        for cause in error.chain() {
            if let Some(error) = cause.downcast_ref::<reqwest::Error>() {
                if error.is_timeout() {
                    return Self::Timeout;
                }
                if let Some(status) = error.status() {
                    return Self::status(status);
                }
                if error.is_decode() {
                    return Self::InvalidOutput;
                }
                if error.is_connect() || error.is_body() {
                    return Self::Transport;
                }
            }
            if let Some(error) =
                cause.downcast_ref::<crate::services::analyzer::ProviderHttpError>()
            {
                return Self::status(error.status);
            }
            if cause.is::<serde_json::Error>() {
                return Self::InvalidOutput;
            }
            if cause.is::<crate::services::analyzer::StreamCut>() {
                return Self::StreamInterrupted;
            }
            if cause.is::<crate::services::ai_quota::AiQuotaError>() {
                return Self::Quota;
            }
        }
        Self::Unknown
    }
}

/// A call to make: whose voice, billed to whom, and what it is called.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ask {
    voice: Voice,
    owner: i32,
    operation: &'static str,
    source: &'static str,
    timeout: Duration,
}

impl Ask {
    /// Billed to `owner` under Merope, as `operation`.
    pub(super) fn new(voice: Voice, owner: i32, operation: &'static str) -> Self {
        Self {
            voice,
            owner,
            operation,
            source: "merope",
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub(super) fn within(self, timeout: Duration) -> Self {
        Self { timeout, ..self }
    }

    /// Billed under another source than Merope.
    pub(super) fn billed_as(self, source: &'static str) -> Self {
        Self { source, ..self }
    }

    /// The configured model, or an observable configuration failure.
    pub(super) async fn model(self) -> Result<Model, Failure> {
        let analyzer = match self.voice {
            Voice::Judge => {
                crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(self.timeout))
                    .await
                    .ok_or_else(|| self.failed(Failure::Unavailable))?
            }
            Voice::Hers => {
                crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(self.timeout))
                    .await
                    .ok_or_else(|| self.failed(Failure::Unavailable))?
                    .with_light_thinking()
            }
            Voice::HersAtLength => {
                crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(self.timeout))
                    .await
                    .ok_or_else(|| self.failed(Failure::Unavailable))?
            }
        };
        Ok(Model {
            analyzer,
            ask: self,
        })
    }

    fn failed(self, failure: Failure) -> Failure {
        tracing::warn!(owner = self.owner, operation = self.operation, source = self.source,
            outcome = %failure, retryable = failure.retryable(), "[Merope] model call failed");
        failure
    }

    /// Raw JSON; failures retain their class for retry and observability.
    pub(super) async fn json_raw(
        self,
        system: &str,
        input: &str,
        schema_name: &str,
        schema: &Value,
    ) -> Result<String, Failure> {
        self.model()
            .await?
            .json(system, input, schema_name, schema)
            .await
    }

    /// [`Ask::json_raw`], read as `T`.
    pub(super) async fn json<T: for<'de> Deserialize<'de>>(
        self,
        system: &str,
        input: &str,
        schema_name: &str,
        schema: &Value,
    ) -> Result<T, Failure> {
        parse(&self.json_raw(system, input, schema_name, schema).await?)
            .ok_or_else(|| self.failed(Failure::InvalidOutput))
    }
}

/// A model ready for one kind of call, billed as its [`Ask`] says.
pub(super) struct Model {
    analyzer: AiAnalyzer,
    ask: Ask,
}

impl Model {
    async fn billed(
        &self,
        call: impl std::future::Future<Output = anyhow::Result<String>>,
    ) -> Result<String, Failure> {
        let result = tokio::time::timeout(
            self.ask.timeout,
            crate::services::ai_cost_ledger::with_site_ai_ledger(
                self.ask.owner,
                self.ask.source,
                self.ask.operation,
                call,
            ),
        )
        .await;
        match result {
            Ok(Ok(raw)) => Ok(raw),
            Ok(Err(error)) => Err(self.ask.failed(Failure::classify(&error))),
            Err(_) => Err(self.ask.failed(Failure::Timeout)),
        }
    }

    /// A JSON answer to `schema`.
    pub(super) async fn json(
        &self,
        system: &str,
        input: &str,
        schema_name: &str,
        schema: &Value,
    ) -> Result<String, Failure> {
        self.billed(
            self.analyzer
                .analyze_json(system, input, schema_name, Some(schema)),
        )
        .await
    }

    /// Free text.
    pub(super) async fn text(&self, system: &str, prompt: &str) -> Result<String, Failure> {
        self.billed(self.analyzer.analyze_with_system(system, prompt))
            .await
    }

    /// What she says to a full speaking prompt, as her chat replies are
    /// asked for.
    pub(super) async fn say(&self, prompt: &str) -> Result<String, Failure> {
        self.billed(
            self.analyzer
                .analyze_stream_parts_with_images(prompt, &[], |_| async { true }),
        )
        .await
    }

    /// What she makes of pictures, asked in `prompt`.
    pub(super) async fn look(
        &self,
        prompt: &str,
        pictures: &[crate::services::analyzer::ImageInput],
    ) -> Result<String, Failure> {
        self.billed(
            self.analyzer
                .analyze_stream_parts_with_images(prompt, pictures, |_| async { true }),
        )
        .await
    }
}

/// Who pays for what she does on her own, with no one asking: the site
/// owner. None when the site has no owner to bill.
pub(super) async fn site_owner() -> Option<i32> {
    crate::services::ai_cost_ledger::resolve_site_owner_id()
        .await
        .ok()
}

/// Whether the provider dropped a reply halfway.
pub(super) fn was_cut(error: &Failure) -> bool {
    *error == Failure::StreamInterrupted
}

/// A model's answer as `T`: the JSON object in it, or the whole of it.
pub(super) use myriad_merope::answer::parse;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn transient_transport_and_provider_statuses_remain_distinct() {
        use axum::{Router, routing::get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route(
                        "/slow",
                        get(|| async {
                            tokio::time::sleep(Duration::from_millis(200)).await;
                            "ok"
                        }),
                    )
                    .route(
                        "/denied",
                        get(|| async { axum::http::StatusCode::UNAUTHORIZED }),
                    )
                    .route(
                        "/limited",
                        get(|| async { axum::http::StatusCode::TOO_MANY_REQUESTS }),
                    )
                    .route(
                        "/broken",
                        get(|| async { axum::http::StatusCode::SERVICE_UNAVAILABLE }),
                    ),
            )
            .await
            .unwrap();
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        let error = client
            .get(format!("http://{address}/slow"))
            .send()
            .await
            .unwrap_err();
        assert_eq!(
            Failure::classify(&anyhow::Error::from(error).context("outer")),
            Failure::Timeout
        );
        for (path, expected) in [
            ("denied", Failure::Rejected),
            ("limited", Failure::RateLimited),
            ("broken", Failure::Upstream),
        ] {
            let error = client
                .get(format!("http://{address}/{path}"))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap_err();
            assert_eq!(Failure::classify(&error.into()), expected);
        }
        server.abort();
        let _ = server.await;
        for failure in [
            Failure::Rejected,
            Failure::RateLimited,
            Failure::Quota,
            Failure::Unavailable,
            Failure::InvalidOutput,
            Failure::Unknown,
        ] {
            assert!(!failure.retryable());
        }
        assert!(Failure::Timeout.retryable());
        assert!(Failure::Transport.retryable());
        assert!(Failure::Upstream.retryable());
        assert!(Failure::StreamInterrupted.retryable());
    }

    #[test]
    fn a_call_is_billed_to_merope_unless_told_otherwise() {
        let ask = Ask::new(Voice::Judge, 7, "touch_appraise");
        assert_eq!(ask.source, "merope");
        assert_eq!(ask.timeout, DEFAULT_TIMEOUT);
        let ask = ask.billed_as("touch").within(Duration::from_secs(2));
        assert_eq!((ask.source, ask.timeout), ("touch", Duration::from_secs(2)));
    }
}
