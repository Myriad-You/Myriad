// AI analysis service using Google Gemini API or OpenAI-compatible API
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::services::http_client::{GeminiApiUrl, ProxyConfig};

// Gemini API Structures
#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    /// Only set for structured-output requests; omitted otherwise so ordinary
    /// calls keep their exact previous request body.
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
}

#[derive(Debug, Serialize)]
struct GeminiGenerationConfig {
    #[serde(rename = "responseMimeType")]
    response_mime_type: String,
    #[serde(rename = "responseSchema", skip_serializing_if = "Option::is_none")]
    response_schema: Option<serde_json::Value>,
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

// OpenAI-compatible API Structures
#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    messages: Vec<OpenAIMessage>,
    /// Only set for structured-output requests; omitted otherwise so ordinary
    /// calls keep their exact previous request body.
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<serde_json::Value>,
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

/// Format a failed OpenAI-compatible HTTP response for user-facing errors.
///
/// The transport is OpenAI-compatible (OpenRouter / DeepSeek / custom gateways),
/// not necessarily official OpenAI — keep the wording accurate so region blocks
/// and wrong models are not misread as "using OpenAI".
fn format_openai_compatible_http_error(
    status: reqwest::StatusCode,
    endpoint: &str,
    model: &str,
    body: &str,
) -> String {
    let body = body.trim();
    let region_blocked = body
        .to_ascii_lowercase()
        .contains("not available in your region")
        || body.contains("\"code\":403")
        || body.contains("\"code\": 403");

    let mut msg = format!(
        "OpenAI-compatible API error {status} (endpoint: {endpoint}, model: {model}): {body}"
    );

    if region_blocked {
        msg.push_str(
            " — this is a provider geo-restriction on the server egress IP (common with OpenRouter for Claude / Grok / GPT / Gemini), not a wrong provider switch. Switch to a region-available model, or enable an outbound proxy in settings.",
        );
    }

    msg
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

// AI Provider Enum
#[derive(Debug, Clone, PartialEq)]
pub enum AiProvider {
    Gemini,
    OpenAI,
}

impl AiProvider {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "openai" | "openrouter" => Self::OpenAI,
            _ => Self::Gemini,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenAI => "openai",
            Self::Gemini => "gemini",
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

fn require_analyze_prompt(profile_data: &serde_json::Value) -> Result<&str> {
    profile_data
        .get("prompt")
        .and_then(|p| p.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("analyze_profile requires a non-empty prompt"))
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

/// How a JSON request is expressed to the provider.
#[derive(Clone, Copy)]
enum JsonMode<'a> {
    /// Use the provider's native structured-output parameters.
    Structured(Option<&'a serde_json::Value>),
    /// Compatibility fallback for endpoints that reject those parameters: no
    /// structured-output fields, with the JSON contract restated in the prompt
    /// the way the pre-existing call sites did it.
    PromptOnly(Option<&'a serde_json::Value>),
}

impl JsonMode<'_> {
    fn decorate_prompt(self, prompt: &str) -> String {
        let Self::PromptOnly(schema) = self else {
            return prompt.to_string();
        };
        let mut text = prompt.to_string();
        text.push_str("\n\nReturn one valid JSON value only, without Markdown fences.");
        if let Some(schema) = schema {
            text.push_str(" The JSON value must satisfy this schema:\n");
            text.push_str(&schema.to_string());
        }
        text
    }

    fn gemini_generation_config(self) -> Option<GeminiGenerationConfig> {
        match self {
            Self::Structured(schema) => Some(GeminiGenerationConfig {
                response_mime_type: "application/json".to_string(),
                response_schema: schema.and_then(gemini_response_schema),
            }),
            Self::PromptOnly(_) => None,
        }
    }

    fn openai_response_format(self, schema_name: &str) -> Option<serde_json::Value> {
        match self {
            // `strict` stays false: strict mode forbids free-form objects, and
            // planner step `params` is exactly that. Non-strict still guarantees
            // syntactically valid JSON and guides the shape.
            Self::Structured(Some(schema)) => Some(serde_json::json!({
                "type": "json_schema",
                "json_schema": {
                    "name": schema_name,
                    "schema": schema,
                    "strict": false,
                },
            })),
            Self::Structured(None) => Some(serde_json::json!({ "type": "json_object" })),
            Self::PromptOnly(_) => None,
        }
    }
}

/// A failed provider call, keeping the HTTP status so the caller can tell a
/// rejected request shape from a transport or upstream failure.
struct ProviderCallFailure {
    status: Option<reqwest::StatusCode>,
    error: anyhow::Error,
}

impl ProviderCallFailure {
    fn http(status: reqwest::StatusCode, error: anyhow::Error) -> Self {
        Self {
            status: Some(status),
            error,
        }
    }

    fn transport(error: anyhow::Error) -> Self {
        Self {
            status: None,
            error,
        }
    }

    /// Whether the endpoint refused the request as written. Structured-output
    /// parameters are the only thing that path adds, so a retry without them is
    /// worth one round trip. Auth and rate-limit failures are excluded — they
    /// would fail identically the second time.
    fn rejected_request(&self) -> bool {
        self.status.is_some_and(|status| {
            status.is_client_error()
                && !matches!(
                    status,
                    reqwest::StatusCode::UNAUTHORIZED
                        | reqwest::StatusCode::FORBIDDEN
                        | reqwest::StatusCode::TOO_MANY_REQUESTS
                )
        })
    }
}

/// Keys Gemini accepts inside `responseSchema` (an OpenAPI 3.0 subset).
/// Anything else — `additionalProperties`, `$schema`, `default`, `const` — is
/// rejected with a 400, so unknown keys are dropped rather than forwarded.
const GEMINI_SCHEMA_KEYS: &[&str] = &[
    "type",
    "format",
    "description",
    "nullable",
    "enum",
    "items",
    "properties",
    "required",
    "minItems",
    "maxItems",
];

/// Translate a JSON Schema into Gemini's `responseSchema` dialect.
///
/// Returns `None` when the schema cannot be expressed, in which case the caller
/// keeps `responseMimeType` (still guaranteeing valid JSON) and drops the
/// schema. The main inexpressible case is a free-form object: Gemini rejects an
/// `OBJECT` node without `properties`, which is how open maps like a planner
/// step's `params` are declared.
fn gemini_response_schema(schema: &serde_json::Value) -> Option<serde_json::Value> {
    let map = schema.as_object()?;

    let declared_type = map.get("type").and_then(serde_json::Value::as_str);
    if declared_type == Some("object")
        && map
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .is_none_or(|properties| properties.is_empty())
    {
        return None;
    }

    let mut translated = serde_json::Map::new();
    for (key, value) in map {
        if !GEMINI_SCHEMA_KEYS.contains(&key.as_str()) {
            continue;
        }
        let translated_value = match key.as_str() {
            // Gemini's Type enum is upper case (STRING / OBJECT / ARRAY / ...).
            "type" => serde_json::Value::String(value.as_str()?.to_uppercase()),
            "items" => gemini_response_schema(value)?,
            "properties" => {
                let mut properties = serde_json::Map::new();
                for (name, property) in value.as_object()? {
                    properties.insert(name.clone(), gemini_response_schema(property)?);
                }
                serde_json::Value::Object(properties)
            }
            _ => value.clone(),
        };
        translated.insert(key.clone(), translated_value);
    }
    Some(serde_json::Value::Object(translated))
}

pub(crate) fn openai_chat_completions_url(base_url: Option<&str>) -> String {
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
    /// Cap Gemini response bodies (success + error) to avoid unbounded buffers.
    async fn read_limited_json<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
        max_bytes: usize,
    ) -> anyhow::Result<T> {
        let bytes = crate::services::outbound_security::read_limited_body(response, max_bytes)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        serde_json::from_slice(&bytes).context("Failed to parse Gemini JSON")
    }

    async fn read_limited_error_text(response: reqwest::Response) -> String {
        crate::services::outbound_security::read_limited_body(response, 64 * 1024)
            .await
            .map(|b| String::from_utf8_lossy(&b).to_string())
            .unwrap_or_else(|_| "Unknown error".to_string())
    }

    pub async fn new(
        provider: AiProvider,
        api_key: String,
        model: String,
        base_url: Option<String>,
    ) -> Self {
        Self::new_with_timeout(provider, api_key, model, base_url, Duration::from_secs(120)).await
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

        let builder = Client::builder()
            .timeout(request_timeout)
            .connect_timeout(Duration::from_secs(30))
            .user_agent("Myriad/1.0");

        // MYR-019: if proxy is required and build fails, do not silently direct-connect.
        let client = match crate::services::http_client::apply_proxy(builder, &proxy_config)
            .and_then(|b| b.build())
        {
            Ok(client) => client,
            Err(e) if crate::services::http_client::proxy_is_required(&proxy_config) => {
                tracing::error!(
                    %e,
                    proxy_url = ?proxy_config.proxy_url.as_deref(),
                    "AiAnalyzer: configured outbound proxy failed to build; refusing \
                     silent direct-connect (MYR-019 fail-closed)"
                );
                panic!(
                    "AiAnalyzer: configured outbound proxy failed to build \
                     (fail-closed, no direct bypass): {e}"
                );
            }
            Err(e) => {
                tracing::error!(
                    %e,
                    "AiAnalyzer client build failed with proxy disabled; using direct client"
                );
                Client::builder()
                    .timeout(request_timeout)
                    .connect_timeout(Duration::from_secs(30))
                    .user_agent("Myriad/1.0")
                    .build()
                    .unwrap_or_else(|_| Client::new())
            }
        };
        Self {
            client,
            provider,
            api_key,
            model,
            base_url,
        }
    }

    pub async fn analyze_profile(&self, profile_data: &serde_json::Value) -> Result<String> {
        let prompt = require_analyze_prompt(profile_data)?;
        let input_chars = serde_json::to_string(profile_data)
            .map(|s| s.len())
            .unwrap_or(0);
        let result = match self.provider {
            AiProvider::Gemini => self.analyze_with_gemini(prompt).await,
            AiProvider::OpenAI => self.analyze_with_openai(prompt).await,
        };
        self.note_ledger(input_chars, &result, "analyze").await;
        result
    }

    /// Best-effort site-wide cost ledger. Writes even without attribution
    /// (`source=internal`). Governed Tapp+scheduler paths suppress this hook.
    async fn note_ledger(
        &self,
        input_chars: usize,
        result: &Result<String, anyhow::Error>,
        operation: &str,
    ) {
        let _ = operation;
        match result {
            Ok(text) => {
                crate::services::ai_cost_ledger::record_ai_call_from_attribution(
                    self.provider.as_str(),
                    &self.model,
                    input_chars,
                    text.len(),
                    "completed",
                    None,
                )
                .await;
            }
            Err(_) => {
                crate::services::ai_cost_ledger::record_ai_call_from_attribution(
                    self.provider.as_str(),
                    &self.model,
                    input_chars,
                    0,
                    "failed",
                    Some("AI_PROVIDER_ERROR"),
                )
                .await;
            }
        }
    }

    async fn analyze_with_gemini(&self, prompt: &str) -> Result<String> {
        let request_body = GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart {
                    text: prompt.to_string(),
                }],
            }],
            generation_config: None,
        };

        let url = GeminiApiUrl::generate_content_url(&self.model).await;

        tracing::info!("🔗 Calling Gemini API (model: {})", self.model);

        let response = self
            .client
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to Gemini API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = Self::read_limited_error_text(response).await;
            return Err(anyhow::anyhow!(
                "Gemini API error {}: {}",
                status,
                error_text
            ));
        }

        let gemini_response: GeminiResponse =
            Self::read_limited_json(response, 2 * 1024 * 1024).await?;

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

    async fn analyze_with_openai(&self, prompt: &str) -> Result<String> {
        let request_body = OpenAIRequest {
            model: self.model.clone(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            response_format: None,
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
            .with_context(|| {
                format!(
                    "Failed to send request to OpenAI-compatible API (endpoint: {url}, model: {})",
                    self.model
                )
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = Self::read_limited_error_text(response).await;
            return Err(anyhow::anyhow!(format_openai_compatible_http_error(
                status,
                &url,
                &self.model,
                &error_text,
            )));
        }

        let openai_response: OpenAIResponse =
            Self::read_limited_json(response, 2 * 1024 * 1024).await?;

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
                // analyze_profile notes the ledger once.
                let full_prompt = flatten_messages_for_gemini(system, &messages);
                let data = serde_json::json!({ "prompt": full_prompt });
                self.analyze_profile(&data).await
            }
            AiProvider::OpenAI => {
                let input_chars =
                    system.len() + messages.iter().map(|m| m.content.len()).sum::<usize>();
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
                    response_format: None,
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
                    .with_context(|| {
                        format!(
                            "Failed to send request to OpenAI-compatible API (endpoint: {url}, model: {})",
                            self.model
                        )
                    });

                let result = match response {
                    Ok(response) => {
                        if !response.status().is_success() {
                            let status = response.status();
                            let error_text = Self::read_limited_error_text(response).await;
                            Err(anyhow::anyhow!(format_openai_compatible_http_error(
                                status,
                                &url,
                                &self.model,
                                &error_text,
                            )))
                        } else {
                            let openai_response: OpenAIResponse =
                                Self::read_limited_json(response, 2 * 1024 * 1024).await?;
                            extract_openai_completion_text(&openai_response)
                        }
                    }
                    Err(e) => Err(e),
                };
                self.note_ledger(input_chars, &result, "chat").await;
                result
            }
        }
    }

    /// Ask the provider for JSON using its native structured-output parameters.
    ///
    /// The alternative — telling the model "return one valid JSON value only" in
    /// the prompt and then scraping the first `{` to the last `}` out of free
    /// text — fails whenever the model wraps its answer in prose or a Markdown
    /// fence, and the caller cannot tell a malformed answer from a deliberate
    /// one. Both providers can enforce JSON at the API level instead:
    ///
    /// - OpenAI-compatible: `response_format`, carrying `json_schema` when a
    ///   schema is supplied and `json_object` otherwise.
    /// - Gemini: `generationConfig.responseMimeType`, plus `responseSchema`
    ///   when the schema is expressible in its dialect.
    ///
    /// `schema` is advisory: a schema that cannot be translated (Gemini) is
    /// dropped while JSON enforcement is kept. Gateways that reject the
    /// parameters outright fall back to a plain prompt-only call, so an older or
    /// non-conforming OpenAI-compatible endpoint degrades to previous behaviour
    /// rather than failing the request.
    ///
    /// The returned string is still parsed by the caller — JSON validity is
    /// enforced, matching the caller's own type is not.
    pub async fn analyze_json(
        &self,
        system: &str,
        prompt: &str,
        schema_name: &str,
        schema: Option<&serde_json::Value>,
    ) -> Result<String> {
        let input_chars = system.len() + prompt.len();

        let mut result = self
            .analyze_json_inner(system, prompt, schema_name, JsonMode::Structured(schema))
            .await;

        if let Err(ref failure) = result {
            if failure.rejected_request() {
                tracing::warn!(
                    provider = self.provider.as_str(),
                    model = %self.model,
                    status = ?failure.status,
                    "[AiAnalyzer] Endpoint rejected structured output; retrying prompt-only"
                );
                result = self
                    .analyze_json_inner(system, prompt, schema_name, JsonMode::PromptOnly(schema))
                    .await;
            }
        }

        let result = result.map_err(|failure| failure.error);
        self.note_ledger(input_chars, &result, "structured").await;
        result
    }

    async fn analyze_json_inner(
        &self,
        system: &str,
        prompt: &str,
        schema_name: &str,
        mode: JsonMode<'_>,
    ) -> std::result::Result<String, ProviderCallFailure> {
        match self.provider {
            AiProvider::Gemini => {
                // This client has no native multi-turn support for Gemini; the
                // system prompt is folded in exactly as `analyze_with_messages`
                // does, so behaviour matches the non-structured path.
                let text = flatten_messages_for_gemini(
                    system,
                    &[ChatMessage::user(mode.decorate_prompt(prompt))],
                );

                let request_body = GeminiRequest {
                    contents: vec![GeminiContent {
                        parts: vec![GeminiPart { text }],
                    }],
                    generation_config: mode.gemini_generation_config(),
                };

                let url = GeminiApiUrl::generate_content_url(&self.model).await;
                let response = self
                    .client
                    .post(&url)
                    .header("x-goog-api-key", &self.api_key)
                    .json(&request_body)
                    .send()
                    .await
                    .map_err(|e| ProviderCallFailure::transport(e.into()))?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = Self::read_limited_error_text(response).await;
                    return Err(ProviderCallFailure::http(
                        status,
                        anyhow::anyhow!("Gemini API error {}: {}", status, error_text),
                    ));
                }

                let gemini_response: GeminiResponse =
                    Self::read_limited_json(response, 2 * 1024 * 1024)
                        .await
                        .map_err(ProviderCallFailure::transport)?;

                if let Some(reason) = gemini_response
                    .prompt_feedback
                    .as_ref()
                    .and_then(|feedback| feedback.block_reason.as_ref())
                {
                    return Err(ProviderCallFailure::transport(anyhow::anyhow!(
                        "Gemini blocked the request (reason: {})",
                        reason
                    )));
                }

                gemini_response
                    .candidates
                    .first()
                    .and_then(|c| c.content.as_ref())
                    .and_then(|c| c.parts.first())
                    .map(|p| p.text.clone())
                    .ok_or_else(|| {
                        let reason = gemini_response
                            .candidates
                            .first()
                            .and_then(|c| c.finish_reason.as_deref())
                            .unwrap_or("UNKNOWN");
                        ProviderCallFailure::transport(anyhow::anyhow!(
                            "Gemini returned no content (finishReason: {})",
                            reason
                        ))
                    })
            }
            AiProvider::OpenAI => {
                let mut messages = Vec::with_capacity(2);
                if !system.trim().is_empty() {
                    messages.push(OpenAIMessage {
                        role: "system".to_string(),
                        content: system.to_string(),
                    });
                }
                messages.push(OpenAIMessage {
                    role: "user".to_string(),
                    content: mode.decorate_prompt(prompt),
                });

                let request_body = OpenAIRequest {
                    model: self.model.clone(),
                    messages,
                    response_format: mode.openai_response_format(schema_name),
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
                    .map_err(|e| ProviderCallFailure::transport(e.into()))?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = Self::read_limited_error_text(response).await;
                    return Err(ProviderCallFailure::http(
                        status,
                        anyhow::anyhow!(format_openai_compatible_http_error(
                            status,
                            &url,
                            &self.model,
                            &error_text,
                        )),
                    ));
                }

                let openai_response: OpenAIResponse =
                    Self::read_limited_json(response, 2 * 1024 * 1024)
                        .await
                        .map_err(ProviderCallFailure::transport)?;
                extract_openai_completion_text(&openai_response)
                    .map_err(ProviderCallFailure::transport)
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
    pub async fn analyze_stream<F>(&self, prompt: &str, on_token: F) -> Result<String>
    where
        F: FnMut(&str) -> bool + Send,
    {
        let input_chars = prompt.len();
        let result = self.analyze_stream_inner(prompt, on_token).await;
        self.note_ledger(input_chars, &result, "stream").await;
        result
    }

    async fn analyze_stream_inner<F>(&self, prompt: &str, mut on_token: F) -> Result<String>
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
                    generation_config: None,
                };

                let base_url = crate::services::http_client::GeminiApiUrl::get_base().await;
                let url = format!(
                    "{}/v1beta/models/{}:streamGenerateContent?alt=sse",
                    base_url, self.model
                );

                let mut response = self
                    .client
                    .post(&url)
                    .header("x-goog-api-key", &self.api_key)
                    .json(&request_body)
                    .send()
                    .await
                    .context("Failed to send streaming request to Gemini API")?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = Self::read_limited_error_text(response).await;
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
                    .with_context(|| {
                        format!(
                            "Failed to send streaming request to OpenAI-compatible API (endpoint: {url}, model: {})",
                            self.model
                        )
                    })?;

                if !response.status().is_success() {
                    let status = response.status();
                    let error_text = Self::read_limited_error_text(response).await;
                    return Err(anyhow::anyhow!(format_openai_compatible_http_error(
                        status,
                        &url,
                        &self.model,
                        &error_text,
                    )));
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
        extract_openai_completion_text, flatten_messages_for_gemini,
        format_openai_compatible_http_error, gemini_response_schema, openai_chat_completions_url,
        require_analyze_prompt, ChatMessage, GeminiContent, GeminiPart, GeminiRequest, JsonMode,
        OpenAIMessage, OpenAIRequest, OpenAIResponse, ProviderCallFailure,
    };
    use serde_json::json;

    fn closed_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "score": { "type": "number", "minimum": 0.0 },
                "tags": { "type": "array", "items": { "type": "string" } },
                "kind": { "type": "string", "enum": ["a", "b"] }
            },
            "required": ["summary"],
            "additionalProperties": false
        })
    }

    #[test]
    fn analyze_profile_requires_a_non_empty_prompt() {
        assert_eq!(
            require_analyze_prompt(&json!({ "prompt": "write the card" })).unwrap(),
            "write the card"
        );
        assert!(require_analyze_prompt(&json!({})).is_err());
        assert!(require_analyze_prompt(&json!({ "prompt": "" })).is_err());
        assert!(require_analyze_prompt(&json!({ "summary": "raw profile" })).is_err());
    }

    // Structured output — Gemini schema dialect

    #[test]
    fn gemini_schema_uppercases_types_and_drops_unsupported_keys() {
        let translated = gemini_response_schema(&closed_schema()).expect("closed schema");
        assert_eq!(translated["type"], "OBJECT");
        assert_eq!(translated["properties"]["summary"]["type"], "STRING");
        assert_eq!(translated["properties"]["tags"]["type"], "ARRAY");
        assert_eq!(translated["properties"]["tags"]["items"]["type"], "STRING");
        assert_eq!(translated["required"], json!(["summary"]));
        // Gemini rejects these outright.
        assert!(translated.get("additionalProperties").is_none());
        assert!(translated["properties"]["score"].get("minimum").is_none());
        // enum is part of the accepted subset.
        assert_eq!(translated["properties"]["kind"]["enum"], json!(["a", "b"]));
    }

    #[test]
    fn gemini_schema_rejects_free_form_objects() {
        // Gemini has no way to express an OBJECT without properties, which is
        // how a planner step's open `params` map is declared. The whole schema
        // must be dropped rather than silently losing the field.
        assert!(gemini_response_schema(&json!({ "type": "object" })).is_none());
        assert!(gemini_response_schema(&json!({
            "type": "object",
            "properties": {}
        }))
        .is_none());
        assert!(gemini_response_schema(&json!({
            "type": "object",
            "properties": {
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": { "params": { "type": "object" } }
                    }
                }
            }
        }))
        .is_none());
    }

    #[test]
    fn gemini_schema_rejects_non_object_roots() {
        assert!(gemini_response_schema(&json!("string")).is_none());
        assert!(gemini_response_schema(&json!([1, 2])).is_none());
    }

    // Structured output — request bodies

    #[test]
    fn structured_mode_sets_json_mime_and_schema_for_gemini() {
        let schema = closed_schema();
        let config = JsonMode::Structured(Some(&schema))
            .gemini_generation_config()
            .expect("structured mode configures generation");
        assert_eq!(config.response_mime_type, "application/json");
        assert_eq!(
            config.response_schema.expect("translatable schema")["type"],
            "OBJECT"
        );
    }

    #[test]
    fn untranslatable_schema_still_enforces_json_on_gemini() {
        let schema = json!({ "type": "object", "properties": { "p": { "type": "object" } } });
        let config = JsonMode::Structured(Some(&schema))
            .gemini_generation_config()
            .expect("json mime is kept");
        assert_eq!(config.response_mime_type, "application/json");
        assert!(config.response_schema.is_none());
    }

    #[test]
    fn openai_structured_mode_uses_json_schema_non_strict() {
        let schema = closed_schema();
        let format = JsonMode::Structured(Some(&schema))
            .openai_response_format("planner_output")
            .expect("response_format is set");
        assert_eq!(format["type"], "json_schema");
        assert_eq!(format["json_schema"]["name"], "planner_output");
        // strict mode forbids free-form objects, which planner params require.
        assert_eq!(format["json_schema"]["strict"], json!(false));
        assert_eq!(format["json_schema"]["schema"], schema);
    }

    #[test]
    fn openai_falls_back_to_json_object_without_a_schema() {
        let format = JsonMode::Structured(None)
            .openai_response_format("x")
            .expect("json mode is still requested");
        assert_eq!(format, json!({ "type": "json_object" }));
    }

    #[test]
    fn prompt_only_mode_sends_no_structured_parameters() {
        let schema = closed_schema();
        let mode = JsonMode::PromptOnly(Some(&schema));
        assert!(mode.gemini_generation_config().is_none());
        assert!(mode.openai_response_format("x").is_none());
    }

    #[test]
    fn prompt_only_mode_restates_the_contract_in_the_prompt() {
        let schema = closed_schema();
        let decorated = JsonMode::PromptOnly(Some(&schema)).decorate_prompt("do the thing");
        assert!(decorated.starts_with("do the thing"));
        assert!(decorated.contains("Return one valid JSON value only"));
        assert!(decorated.contains("\"summary\""));

        // Structured mode leaves the prompt untouched; the API enforces it.
        assert_eq!(
            JsonMode::Structured(Some(&schema)).decorate_prompt("do the thing"),
            "do the thing"
        );
    }

    #[test]
    fn ordinary_requests_omit_the_structured_fields_entirely() {
        // Existing call sites must serialize byte-identically to before.
        let gemini = serde_json::to_value(GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart {
                    text: "hi".to_string(),
                }],
            }],
            generation_config: None,
        })
        .expect("serialize");
        assert!(gemini.get("generationConfig").is_none());

        let openai = serde_json::to_value(OpenAIRequest {
            model: "gpt-x".to_string(),
            messages: vec![OpenAIMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
            }],
            response_format: None,
        })
        .expect("serialize");
        assert!(openai.get("response_format").is_none());
    }

    // Structured output — compatibility fallback

    #[test]
    fn rejected_request_triggers_the_prompt_only_retry() {
        for status in [
            reqwest::StatusCode::BAD_REQUEST,
            reqwest::StatusCode::NOT_FOUND,
            reqwest::StatusCode::UNPROCESSABLE_ENTITY,
        ] {
            let failure = ProviderCallFailure::http(status, anyhow::anyhow!("nope"));
            assert!(failure.rejected_request(), "{status} should retry plain");
        }
    }

    #[test]
    fn auth_rate_limit_and_transport_failures_do_not_retry() {
        for status in [
            reqwest::StatusCode::UNAUTHORIZED,
            reqwest::StatusCode::FORBIDDEN,
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            reqwest::StatusCode::BAD_GATEWAY,
        ] {
            let failure = ProviderCallFailure::http(status, anyhow::anyhow!("nope"));
            assert!(
                !failure.rejected_request(),
                "{status} would fail identically on retry"
            );
        }
        assert!(!ProviderCallFailure::transport(anyhow::anyhow!("timeout")).rejected_request());
    }

    #[test]
    fn openai_compatible_error_does_not_claim_official_openai() {
        let msg = format_openai_compatible_http_error(
            reqwest::StatusCode::FORBIDDEN,
            "https://openrouter.ai/api/v1/chat/completions",
            "x-ai/grok-4.5",
            r#"{"error":{"message":"This model is not available in your region.","code":403}}"#,
        );
        assert!(msg.contains("OpenAI-compatible API error"));
        assert!(!msg.starts_with("OpenAI API error"));
        assert!(msg.contains("x-ai/grok-4.5"));
        assert!(msg.contains("openrouter.ai"));
        assert!(msg.contains("geo-restriction"));
    }

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
