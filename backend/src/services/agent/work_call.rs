//! Model transport for Work: a selected model shared by the loop and its tools.
//!
//! The durable loop owns cancellation, deadlines and budget reservations; the
//! request budget and usage ledger remain task-local across this boundary.
//! This wrapper does not expose the analyzer, so handlers cannot bypass it.
use crate::config::ModelTier;
use crate::services::analyzer::{
    AiAnalyzer,
    tool_calling::{ToolDefinition, ToolMessage, ToolTurn},
};

pub(crate) struct WorkModel {
    analyzer: AiAnalyzer,
}

impl WorkModel {
    pub(crate) async fn configured(tier: ModelTier) -> Option<Self> {
        crate::services::ai::create_ai_analyzer_for_tier(tier)
            .await
            .map(|analyzer| Self { analyzer })
    }

    pub(crate) async fn text(&self, prompt: &str) -> anyhow::Result<String> {
        self.analyzer.analyze(prompt).await
    }

    pub(crate) async fn tool_turn<F, Fut>(
        &self,
        system: &str,
        history: &[ToolMessage],
        tools: &[ToolDefinition],
        max_tokens: u32,
        on_text: F,
    ) -> anyhow::Result<ToolTurn>
    where
        F: FnMut(String) -> Fut + Send,
        Fut: std::future::Future<Output = ()> + Send,
    {
        self.analyzer
            .tool_turn(system, history, tools, max_tokens, on_text)
            .await
    }
}

#[cfg(test)]
impl From<AiAnalyzer> for WorkModel {
    fn from(analyzer: AiAnalyzer) -> Self {
        Self { analyzer }
    }
}
