//! Provider-side execution for host-governed AI Tasks.
//!
//! Text (AiAnalyzer) and image (configured providers) live here so the HTTP
//! orchestration module does not own outbound provider logic. Task registry,
//! quota, and local cancel state stay with the caller.

use serde_json::{json, Value};

use crate::services::ai_config::{AiConfig, AiImageConfig};
use crate::services::analyzer::AiAnalyzer;

/// Stable provider error (code + message) shared with the AI Task API surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderError {
    pub code: String,
    pub message: String,
}

impl ProviderError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn into_pair(self) -> (String, String) {
        (self.code, self.message)
    }
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ProviderError {}

/// Parse image width/height: JSON int, whole float, or numeric string (`"768"` / `"768px"`).
pub fn parse_image_dim(value: &Value) -> Option<u32> {
    if let Some(n) = value.as_u64() {
        return u32::try_from(n).ok().filter(|&n| n > 0);
    }
    if let Some(n) = value.as_i64() {
        return u32::try_from(n).ok().filter(|&n| n > 0);
    }
    if let Some(n) = value.as_f64() {
        if n.is_finite() && n > 0.0 && n.fract() == 0.0 && n <= u32::MAX as f64 {
            return Some(n as u32);
        }
        return None;
    }
    if let Some(s) = value.as_str() {
        let s = s.trim();
        let s = s
            .strip_suffix("px")
            .or_else(|| s.strip_suffix("PX"))
            .unwrap_or(s)
            .trim();
        return s.parse::<u32>().ok().filter(|&n| n > 0);
    }
    None
}

/// Read resolution from task input; missing keys use local defaults (not global config).
pub fn image_size_from_input(input: &Value) -> (u32, u32) {
    const DEFAULT_W: u32 = 1024;
    const DEFAULT_H: u32 = 1024;
    const MIN: u32 = 256;
    const MAX: u32 = 2048;
    let width = input
        .get("width")
        .and_then(parse_image_dim)
        .map(|v| v.clamp(MIN, MAX))
        .unwrap_or(DEFAULT_W);
    let height = input
        .get("height")
        .and_then(parse_image_dim)
        .map(|v| v.clamp(MIN, MAX))
        .unwrap_or(DEFAULT_H);
    (width, height)
}

/// Run a text model. When `stream` is true, `on_delta` receives each token chunk.
///
/// Returns `(raw_text, estimated_input_tokens, estimated_output_tokens)`.
pub async fn run_text_provider<F>(
    config: AiConfig,
    system: &str,
    prompt: &str,
    stream: bool,
    mut on_delta: F,
) -> Result<(String, usize, usize), ProviderError>
where
    F: FnMut(&str) -> bool + Send,
{
    let analyzer = AiAnalyzer::new(
        config.provider,
        config.api_key,
        config.model,
        config.base_url,
    )
    .await;
    let raw = if stream {
        analyzer
            .analyze_stream(&format!("{system}\n\n{prompt}"), |delta| on_delta(delta))
            .await
    } else {
        analyzer.analyze_with_system(system, prompt).await
    }
    .map_err(|_| {
        ProviderError::new(
            "AI_PROVIDER_ERROR",
            "AI provider failed to complete the task",
        )
    })?;
    let input_tokens = (system.len() + prompt.len()) / 4;
    let output_tokens = raw.len() / 4;
    Ok((raw, input_tokens, output_tokens))
}

/// Run the configured image provider and persist the result locally.
///
/// `on_progress` is reserved for providers that poll remote jobs.
/// Result value shape: `{ format: "image", value: { url, width, height }, contextProvenance: [] }`.
pub async fn run_image_provider<F>(
    config: AiImageConfig,
    prompt: &str,
    width: u32,
    height: u32,
    mut _on_progress: F,
) -> Result<Value, ProviderError>
where
    F: FnMut(u32, u32) + Send,
{
    let generated = crate::services::image_generation::generate_image(
        &crate::services::image_generation::ImageGenerationConfig {
            provider: config.provider,
            model: config.model,
            api_key: config.api_key,
            base_url: config.base_url,
        },
        prompt,
        width,
        height,
        None,
    )
    .await
    .map_err(|error| ProviderError::new("AI_PROVIDER_ERROR", error.to_string()))?;
    let url = crate::services::image_generation::persist_generated(&generated)
        .await
        .map_err(|error| ProviderError::new("AI_PROVIDER_ERROR", error.to_string()))?;
    Ok(json!({
        "format": "image",
        "value": {
            "url": url,
            "width": generated.width,
            "height": generated.height,
        },
        "contextProvenance": [],
    }))
}

#[cfg(test)]
mod tests {
    use super::{image_size_from_input, parse_image_dim, ProviderError};
    use serde_json::json;

    #[test]
    fn parse_image_dim_accepts_number_and_string() {
        assert_eq!(parse_image_dim(&json!(768)), Some(768));
        assert_eq!(parse_image_dim(&json!(768.0)), Some(768));
        assert_eq!(parse_image_dim(&json!("1024")), Some(1024));
        assert_eq!(parse_image_dim(&json!(" 768px ")), Some(768));
        assert_eq!(parse_image_dim(&json!(0)), None);
        assert_eq!(parse_image_dim(&json!("nope")), None);
    }

    #[test]
    fn image_size_from_input_defaults_clamps_and_parses_strings() {
        assert_eq!(image_size_from_input(&json!({})), (1024, 1024));
        assert_eq!(
            image_size_from_input(&json!({ "width": "768", "height": "1024px" })),
            (768, 1024)
        );
        assert_eq!(
            image_size_from_input(&json!({ "width": 100, "height": 5000 })),
            (256, 2048)
        );
        assert_eq!(image_size_from_input(&json!("a cat")), (1024, 1024));
    }

    #[test]
    fn provider_error_pair_preserves_code() {
        let err = ProviderError::new("AI_PROVIDER_ERROR", "boom");
        let (code, message) = err.into_pair();
        assert_eq!(code, "AI_PROVIDER_ERROR");
        assert_eq!(message, "boom");
    }
}
