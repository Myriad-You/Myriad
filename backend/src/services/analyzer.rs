// AI analysis service using Google Gemini API or OpenAI-compatible API
use anyhow::{Context, Result};
use reqwest::{Client, Proxy};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::services::http_client::{GeminiApiUrl, ProxyConfig};

// ============= Gemini API Structures =============
#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
}

#[derive(Debug, Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    #[serde(default)]
    candidates: Vec<GeminiCandidate>,
    #[serde(default, rename = "promptFeedback")]
    prompt_feedback: Option<GeminiPromptFeedback>,
}

#[derive(Debug, Deserialize)]
struct GeminiPromptFeedback {
    #[serde(default, rename = "blockReason")]
    block_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: Option<GeminiCandidateContent>,
    #[serde(default, rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidateContent {
    #[serde(default)]
    parts: Vec<GeminiResponsePart>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponsePart {
    #[serde(default)]
    text: String,
}

// ============= OpenAI-compatible API Structures =============
#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
}

#[derive(Debug, Clone, Serialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

/// OpenAI-compatible chat role/content pair for multi-turn analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }

    #[allow(dead_code)]
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAIResponse {
    #[serde(default)]
    choices: Vec<OpenAIChoice>,
    /// OpenRouter and some gateways return HTTP 200 with a top-level error object.
    #[serde(default)]
    error: Option<OpenAIErrorBody>,
}

#[derive(Debug, Deserialize)]
struct OpenAIErrorBody {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct OpenAIChoice {
    #[serde(default)]
    message: OpenAIResponseMessage,
}

#[derive(Debug, Deserialize, Default)]
struct OpenAIResponseMessage {
    /// Providers may return `null` for content when only reasoning is filled.
    #[serde(default)]
    content: Option<String>,
    /// OpenRouter / DeepSeek-style reasoning fields (optional, never required).
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
}

/// Extract assistant text from an OpenAI-compatible chat completion body.
///
/// Never panics: returns clear anyhow errors for playground 502 paths.
fn extract_openai_completion_text(response: &OpenAIResponse) -> Result<String> {
    if let Some(error) = &response.error {
        let message = error
            .message
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("OpenAI-compatible provider returned an error object");
        let code = error
            .code
            .as_ref()
            .map(|c| match c {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .filter(|s| !s.is_empty() && s != "null");
        return match code {
            Some(code) => Err(anyhow::anyhow!(
                "OpenAI-compatible API error (code {code}): {message}"
            )),
            None => Err(anyhow::anyhow!("OpenAI-compatible API error: {message}")),
        };
    }

    let message = response
        .choices
        .first()
        .map(|choice| &choice.message)
        .ok_or_else(|| anyhow::anyhow!("OpenAI-compatible API returned no choices"))?;

    if let Some(content) = message
        .content
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return Ok(content.to_string());
    }

    // Prefer non-empty content; fall back to common reasoning fields when
    // providers put the payload only in reasoning_* (still never panic).
    for candidate in [
        message.reasoning_content.as_deref(),
        message.reasoning.as_deref(),
    ] {
        if let Some(text) = candidate.map(str::trim).filter(|s| !s.is_empty()) {
            return Ok(text.to_string());
        }
    }

    Err(anyhow::anyhow!(
        "OpenAI-compatible API returned empty message content"
    ))
}

// ============= AI Provider Enum =============
#[derive(Debug, Clone, PartialEq)]
pub enum AiProvider {
    Gemini,
    OpenAI,
}

impl AiProvider {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "openai" => Self::OpenAI,
            _ => Self::Gemini,
        }
    }
}

pub struct AiAnalyzer {
    client: Client,
    provider: AiProvider,
    api_key: String,
    model: String,
    base_url: Option<String>, // For OpenAI-compatible APIs
}

/// Flatten multi-turn messages into a single Gemini-compatible prompt.
fn flatten_messages_for_gemini(system: &str, messages: &[ChatMessage]) -> String {
    let mut parts = Vec::new();
    if !system.trim().is_empty() {
        parts.push(format!("SYSTEM:\n{system}"));
    }
    for message in messages {
        let label = match message.role.as_str() {
            "assistant" => "ASSISTANT",
            "system" => "SYSTEM",
            _ => "USER",
        };
        parts.push(format!("{label}:\n{}", message.content));
    }
    parts.join("\n\n")
}

fn openai_chat_completions_url(base_url: Option<&str>) -> String {
    let base_url = base_url
        .unwrap_or("https://api.openai.com/v1")
        .trim()
        .trim_end_matches('/');

    if base_url.ends_with("/chat/completions") {
        return base_url.to_string();
    }

    // OpenRouter documents its OpenAI-compatible API under /api/v1. Accepting
    // the site root here makes a common settings mistake safe without changing
    // the semantics of custom OpenAI-compatible endpoints.
    if matches!(base_url, "https://openrouter.ai" | "http://openrouter.ai") {
        return format!("{base_url}/api/v1/chat/completions");
    }

    // The official OpenAI host is the other common root-only value.
    if matches!(base_url, "https://api.openai.com" | "http://api.openai.com") {
        return format!("{base_url}/v1/chat/completions");
    }

    format!("{base_url}/chat/completions")
}

impl AiAnalyzer {
    pub async fn new(
        provider: AiProvider,
        api_key: String,
        model: String,
        base_url: Option<String>,
    ) -> Self {
        Self::new_with_timeout(
            provider,
            api_key,
            model,
            base_url,
            Duration::from_secs(120),
        )
        .await
    }

    /// 与 [`AiAnalyzer::new`] 相同，但允许长任务（如 Tapp Playground 生成）
    /// 指定更长的单次请求超时。
    pub async fn new_with_timeout(
        provider: AiProvider,
        api_key: String,
        model: String,
        base_url: Option<String>,
        request_timeout: Duration,
    ) -> Self {
        let proxy_config = ProxyConfig::from_dynamic_config().await;

        let mut builder = Client::builder()
            .timeout(request_timeout)
            .connect_timeout(Duration::from_secs(30))
            .user_agent("Myriad/1.0");

        if proxy_config.should_use_proxy() {
            if let Some(proxy_url) = proxy_config.proxy_url.as_deref() {
                if let Ok(proxy) = Proxy::all(proxy_url) {
                    builder = builder.proxy(proxy);
                }
            }
        }

        let client = builder.build().unwrap_or_else(|_| Client::new());
        Self {
            client,
            provider,
            api_key,
            model,
            base_url,
        }
    }

    pub async fn analyze_profile(&self, profile_data: &serde_json::Value) -> Result<String> {
        match self.provider {
            AiProvider::Gemini => self.analyze_with_gemini(profile_data).await,
            AiProvider::OpenAI => self.analyze_with_openai(profile_data).await,
        }
    }

    async fn analyze_with_gemini(&self, profile_data: &serde_json::Value) -> Result<String> {
        // 检查是否提供了自定义提示词
        let user_prompt = if let Some(prompt) = profile_data.get("prompt").and_then(|p| p.as_str())
        {
            prompt.to_string()
        } else {
            let system_prompt = "You are an expert data analyst. Analyze data thoroughly and provide structured, actionable insights. Always respond in the same language as the input data.";
            let data_str = serde_json::to_string_pretty(profile_data)?;
            // 截断过长数据以避免 token 溢出
            let truncated: String = data_str.chars().take(15000).collect();
            format!(
                "{}\n\nPlease analyze the following data and provide:\n\
                1. Key findings and patterns\n\
                2. Notable highlights\n\
                3. Actionable insights\n\n\
                Data:\n{}",
                system_prompt, truncated
            )
        };

        let request_body = GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart { text: user_prompt }],
            }],
        };

        let url = GeminiApiUrl::generate_content_url(&self.model).await;

        tracing::info!("🔗 Calling Gemini API (model: {})", self.model);

        let response = self
            .client
            .post(&url)
            .query(&[("key", &self.api_key)])
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to Gemini API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!(
                "Gemini API error {}: {}",
                status,
                error_text
            ));
        }

        let gemini_response: GeminiResponse = response
            .json()
            .await
            .context("Failed to parse Gemini API response")?;

        // 检测 prompt 级别的安全过滤
        if let Some(ref feedback) = gemini_response.prompt_feedback {
            if let Some(ref reason) = feedback.block_reason {
                return Err(anyhow::anyhow!(
                    "Gemini blocked the request (reason: {})",
                    reason
                ));
            }
        }

        // 检测 candidate 级别的安全过滤（有 candidate 但无 content）
        if let Some(candidate) = gemini_response.candidates.first() {
            if candidate.content.is_none() {
                let reason = candidate.finish_reason.as_deref().unwrap_or("UNKNOWN");
                return Err(anyhow::anyhow!(
                    "Gemini blocked the response (finishReason: {})",
                    reason
                ));
            }
        }

        let analysis = gemini_response
            .candidates
            .first()
            .and_then(|c| c.content.as_ref())
            .and_then(|c| c.parts.first())
            .map(|p| p.text.clone())
            .unwrap_or_else(|| "No analysis generated".to_string());

        Ok(analysis)
    }

    async fn analyze_with_openai(&self, profile_data: &serde_json::Value) -> Result<String> {
        // 检查是否提供了自定义提示词
        let user_content = if let Some(prompt) = profile_data.get("prompt").and_then(|p| p.as_str())
        {
            prompt.to_string()
        } else {
            format!(
                "Analyze the following user profile data and provide insights on their professional background, skills, interests, and online presence:\n\n{}",
                serde_json::to_string_pretty(profile_data)?
            )
        };

        let request_body = OpenAIRequest {
            model: self.model.clone(),
            messages: vec![
                OpenAIMessage {
                    role: "system".to_string(),
                    content: "You are an expert data analyst specializing in social media and professional profiles.".to_string(),
                },
                OpenAIMessage {
                    role: "user".to_string(),
                    content: user_content,
                },
            ],
        };

        let url = openai_chat_completions_url(self.base_url.as_deref());

        tracing::info!(
            "🔗 Calling OpenAI-compatible API: {} (model: {})",
            url,
            self.model
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to OpenAI API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!(
                "OpenAI API error {}: {}",
                status,
                error_text
            ));
        }

        let openai_response: OpenAIResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI API response")?;

        extract_openai_completion_text(&openai_response)
    }

    /// 简单的分析方法（用于 Tapp API）
    pub async fn analyze(&self, prompt: &str) -> Result<String> {
        let data = serde_json::json!({ "prompt": prompt });
        self.analyze_profile(&data).await
    }

    /// 带系统提示的分析方法（用于 Tapp API）
    pub async fn analyze_with_system(&self, system: &str, prompt: &str) -> Result<String> {
        self.analyze_with_messages(system, vec![ChatMessage::user(prompt.to_string())])
            .await
    }

    /// Multi-turn analysis: system prompt + ordered role/content messages.
    ///
    /// OpenAI-compatible providers receive true multi-turn `messages`.
    /// Gemini flattens the conversation into a single prompt (no native multi-turn
    /// system+history support in this client).
    pub async fn analyze_with_messages(
        &self,
        system: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<String> {
        match self.provider {
            AiProvider::Gemini => {
                let full_prompt = flatten_messages_for_gemini(system, &messages);
                let data = serde_json::json!({ "prompt": full_prompt });
                self.analyze_profile(&data).await
            }
            AiProvider::OpenAI => {
                let mut openai_messages = Vec::with_capacity(messages.len() + 1);
                if !system.trim().is_empty() {
                    openai_messages.push(OpenAIMessage {
                        role: "system".to_string(),
                        content: system.to_string(),
                    });
                }
                for message in messages {
                    let role = match message.role.as_str() {
                        "assistant" => "assistant",
                        "system" => "system",
                        _ => "user",
                    };
                    openai_messages.push(OpenAIMessage {
                        role: role.to_string(),
                        content: message.content,
                    });
                }

                let request_body = OpenAIRequest {
                    model: self.model.clone(),
                    messages: openai_messages,
                };

                let url = openai_chat_completions_url(self.base_url.as_deref());

                let response = self
                    .client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("Content-Type", "application/json")
                    .json(&request_body)
                    .send()
                    .await
                    .context("Failed to send request to OpenAI API")?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "Unknown error".to_string());
                    return Err(anyhow::anyhow!(
                        "OpenAI API error {}: {}",
                        status,
                        error_text
                    ));
                }

                let openai_response: OpenAIResponse = response
                    .json()
                    .await
                    .context("Failed to parse OpenAI API response")?;

                extract_openai_completion_text(&openai_response)
            }
        }
    }

    // TODO: Add more analysis methods
    // pub async fn generate_summary(&self, profiles: Vec<serde_json::Value>) -> Result<String>
    // pub async fn extract_skills(&self, profile_data: &serde_json::Value) -> Result<Vec<String>>

    /// 流式分析（逐 token 返回）
    ///
    /// 通过 `on_token` 回调逐步返回文本片段，适用于需要实时展示 AI 回复的场景。
    /// 回调返回 `false` 可提前终止流。
    pub async fn analyze_stream<F>(&self, prompt: &str, mut on_token: F) -> Result<String>
    where
        F: FnMut(&str) -> bool + Send,
    {
        let mut full_text = String::new();
        match self.provider {
            AiProvider::Gemini => {
                let request_body = GeminiRequest {
                    contents: vec![GeminiContent {
                        parts: vec![GeminiPart {
                            text: prompt.to_string(),
                        }],
                    }],
                };

                let base_url = crate::services::http_client::GeminiApiUrl::get_base().await;
                let url = format!(
                    "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
                    base_url, self.model
                );

                let mut response = self
                    .client
                    .post(&url)
                    .query(&[("key", &self.api_key)])
                    .json(&request_body)
                    .send()
                    .await
                    .context("Failed to send streaming request to Gemini API")?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = response.text().await.unwrap_or_default();
                    return Err(anyhow::anyhow!(
                        "Gemini streaming API error {}: {}",
                        status,
                        error_text
                    ));
                }

                let mut buffer = String::new();

                loop {
                    let chunk = response.chunk().await.context("Stream read error")?;
                    match chunk {
                        Some(bytes) => buffer.push_str(&String::from_utf8_lossy(&bytes)),
                        None => break,
                    }

                    // Parse SSE lines: "data: {...}\n\n"
                    while let Some(pos) = buffer.find("\n\n") {
                        let event_block = buffer[..pos].to_string();
                        buffer = buffer[pos + 2..].to_string();

                        for line in event_block.lines() {
                            let line = line.trim();
                            if let Some(data) = line.strip_prefix("data: ") {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                                    if let Some(text) = json
                                        .pointer("/candidates/0/content/parts/0/text")
                                        .and_then(|v| v.as_str())
                                    {
                                        full_text.push_str(text);
                                        if !on_token(text) {
                                            return Ok(full_text);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            AiProvider::OpenAI => {
                #[derive(Serialize)]
                struct OpenAIStreamRequest {
                    model: String,
                    messages: Vec<OpenAIMessage>,
                    stream: bool,
                }

                let request_body = OpenAIStreamRequest {
                    model: self.model.clone(),
                    messages: vec![OpenAIMessage {
                        role: "user".to_string(),
                        content: prompt.to_string(),
                    }],
                    stream: true,
                };

                let url = openai_chat_completions_url(self.base_url.as_deref());

                let mut response = self
                    .client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.api_key))
                    .header("Content-Type", "application/json")
                    .json(&request_body)
                    .send()
                    .await
                    .context("Failed to send streaming request to OpenAI API")?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = response.text().await.unwrap_or_default();
                    return Err(anyhow::anyhow!(
                        "OpenAI streaming API error {}: {}",
                        status,
                        error_text
                    ));
                }

                let mut buffer = String::new();

                loop {
                    let chunk = response.chunk().await.context("Stream read error")?;
                    match chunk {
                        Some(bytes) => buffer.push_str(&String::from_utf8_lossy(&bytes)),
                        None => break,
                    }

                    while let Some(pos) = buffer.find('\n') {
                        let line = buffer[..pos].trim().to_string();
                        buffer = buffer[pos + 1..].to_string();

                        if line.is_empty() {
                            continue;
                        }
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data.trim() == "[DONE]" {
                                break;
                            }
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                                if let Some(content) = json
                                    .pointer("/choices/0/delta/content")
                                    .and_then(|v| v.as_str())
                                {
                                    full_text.push_str(content);
                                    if !on_token(content) {
                                        return Ok(full_text);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(full_text)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extract_openai_completion_text, flatten_messages_for_gemini, openai_chat_completions_url,
        ChatMessage, OpenAIResponse,
    };

    #[test]
    fn flattens_multi_turn_messages_for_gemini() {
        let flat = flatten_messages_for_gemini(
            "rules",
            &[
                ChatMessage::user("first"),
                ChatMessage::assistant("reply"),
                ChatMessage::user("second"),
            ],
        );
        assert!(flat.starts_with("SYSTEM:\nrules"));
        assert!(flat.contains("USER:\nfirst"));
        assert!(flat.contains("ASSISTANT:\nreply"));
        assert!(flat.contains("USER:\nsecond"));
    }

    #[test]
    fn normalizes_openai_compatible_chat_urls() {
        assert_eq!(
            openai_chat_completions_url(None),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url(Some("https://api.openai.com")),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url(Some("https://openrouter.ai/")),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url(Some("https://openrouter.ai/api/v1")),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        assert_eq!(
            openai_chat_completions_url(Some("https://gateway.example.com/v1/chat/completions")),
            "https://gateway.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn openai_response_deserializes_null_content() {
        let raw = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "reasoning_content": "step by step: final answer HERE"
                }
            }]
        }"#;
        let parsed: OpenAIResponse = serde_json::from_str(raw).expect("deserialize");
        let text = extract_openai_completion_text(&parsed).expect("fallback to reasoning");
        assert!(text.contains("final answer HERE"));
    }

    #[test]
    fn openai_response_top_level_error_is_err() {
        let raw = r#"{
            "error": {
                "message": "Provider returned error",
                "code": "model_not_found"
            },
            "choices": []
        }"#;
        let parsed: OpenAIResponse = serde_json::from_str(raw).expect("deserialize");
        let err = extract_openai_completion_text(&parsed).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("Provider returned error"));
        assert!(msg.contains("model_not_found"));
    }

    #[test]
    fn openai_response_empty_content_without_reasoning_is_err() {
        let raw = r#"{
            "choices": [{
                "message": { "role": "assistant", "content": null }
            }]
        }"#;
        let parsed: OpenAIResponse = serde_json::from_str(raw).expect("deserialize");
        let err = extract_openai_completion_text(&parsed).unwrap_err();
        assert!(format!("{err:#}").contains("empty message content"));
    }

    #[test]
    fn openai_response_prefers_non_empty_content() {
        let raw = r#"{
            "choices": [{
                "message": {
                    "content": "  visible payload  ",
                    "reasoning_content": "hidden chain"
                }
            }]
        }"#;
        let parsed: OpenAIResponse = serde_json::from_str(raw).expect("deserialize");
        let text = extract_openai_completion_text(&parsed).expect("content");
        assert_eq!(text, "visible payload");
    }
}
