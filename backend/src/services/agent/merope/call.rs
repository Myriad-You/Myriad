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

    /// The model for this call, or none if none is set up.
    pub(super) async fn model(self) -> Option<Model> {
        let analyzer = match self.voice {
            Voice::Judge => {
                crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(self.timeout))
                    .await?
            }
            Voice::Hers => {
                crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(self.timeout))
                    .await?
                    .with_light_thinking()
            }
            Voice::HersAtLength => {
                crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(self.timeout))
                    .await?
            }
        };
        Some(Model {
            analyzer,
            ask: self,
        })
    }

    /// The JSON answer, or none if no model is set up, it failed or timed out.
    pub(super) async fn json_raw(
        self,
        system: &str,
        input: &str,
        schema_name: &str,
        schema: &Value,
    ) -> Option<String> {
        self.model()
            .await?
            .json(system, input, schema_name, schema)
            .await
            .ok()
    }

    /// [`Ask::json_raw`], read as `T`.
    pub(super) async fn json<T: for<'de> Deserialize<'de>>(
        self,
        system: &str,
        input: &str,
        schema_name: &str,
        schema: &Value,
    ) -> Option<T> {
        parse(&self.json_raw(system, input, schema_name, schema).await?)
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
    ) -> anyhow::Result<String> {
        crate::services::ai_cost_ledger::with_site_ai_ledger(
            self.ask.owner,
            self.ask.source,
            self.ask.operation,
            call,
        )
        .await
    }

    /// A JSON answer to `schema`.
    pub(super) async fn json(
        &self,
        system: &str,
        input: &str,
        schema_name: &str,
        schema: &Value,
    ) -> anyhow::Result<String> {
        self.billed(
            self.analyzer
                .analyze_json(system, input, schema_name, Some(schema)),
        )
        .await
    }

    /// Free text.
    pub(super) async fn text(&self, system: &str, prompt: &str) -> anyhow::Result<String> {
        self.billed(self.analyzer.analyze_with_system(system, prompt))
            .await
    }

    /// What she says to a full speaking prompt, as her chat replies are
    /// asked for.
    pub(super) async fn say(&self, prompt: &str) -> anyhow::Result<String> {
        self.billed(
            self.analyzer
                .analyze_stream_parts_with_images(prompt, &[], |_| async { true }),
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
pub(super) fn was_cut(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<crate::services::analyzer::StreamCut>()
        .is_some()
}

/// A model's answer as `T`: the JSON object in it, or the whole of it.
pub(super) fn parse<T: for<'de> Deserialize<'de>>(raw: &str) -> Option<T> {
    let json = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim());
    serde_json::from_str(json.as_deref().unwrap_or(raw.trim())).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_read_from_the_json_in_it() {
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct Pick {
            choice: Option<usize>,
        }
        assert_eq!(
            parse::<Pick>("好的：\n```json\n{\"choice\":2}\n```"),
            Some(Pick { choice: Some(2) })
        );
        assert_eq!(
            parse::<Pick>("{\"choice\":null}"),
            Some(Pick { choice: None })
        );
        assert_eq!(parse::<Pick>("不知道"), None);
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
